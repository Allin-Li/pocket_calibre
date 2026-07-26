mod calibre;
mod config;
mod keyboard;
mod libm_shim;
mod net;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use inkview::Event;
use inkview::bindings::Inkview;
use inkview::screen::Screen;
use slint::{ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, TimerMode, VecModel};

use calibre::{Book, Client};
use config::Config;
use keyboard::Field;

slint::include_modules!();

/// Размер миниатюры, который просим у сервера. Подходит для обоих вероятных
/// масштабов экрана: лишнее Slint ужмёт, а тянуть полную обложку на 230 КБ
/// ради строки списка незачем.
const COVER_SIZE: (u32, u32) = (120, 170);

/// Команды от UI к рабочему потоку.
enum Cmd {
    Refresh,
    Download(i32),
    Reconfigure(Config),
    Covers(Vec<i64>),
}

/// Ответы рабочего потока к UI.
enum Msg {
    Status(String),
    Busy(bool),
    Books(Vec<Book>),
    BookState(i32, &'static str),
    Cover {
        id: i64,
        rgb: Vec<u8>,
        width: u32,
        height: u32,
    },
}

fn main() {
    // Inkview живёт всё время работы процесса, а Screen и рабочий поток хотят
    // ссылку с 'static — отсюда утечка вместо возни с Arc.
    let iv: &'static Inkview = Box::leak(Box::new(inkview::load()));
    let (evt_tx, evt_rx) = mpsc::channel();
    let (redraw_tx, redraw_rx) = mpsc::channel();

    std::thread::Builder::new()
        .name("ui".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || ui_main(iv, evt_rx, redraw_rx))
        .expect("не удалось запустить UI-поток");

    inkview::iv_main(iv, move |event| {
        // Пока открыта нативная клавиатура, ввод принадлежит ей: пробрасывать
        // тапы в Slint значило бы жать кнопки под клавиатурой.
        if keyboard::is_open(iv) {
            return Some(());
        }

        // PointerMove не пробрасываем. Палец генерирует их десятками в секунду,
        // а цикл бэкенда обрабатывает одно событие за кадр, где кадр — это
        // полная отрисовка плюс обновление e-ink. Очередь росла быстрее, чем
        // разгребалась, и интерфейс уползал за пальцем. Для тапа эти события не
        // нужны: press и release несут свои координаты.
        if matches!(event, Event::PointerMove { .. }) {
            return Some(());
        }

        // Бэкенд inkview-slint эти события игнорирует, а между тем именно они
        // означают «кадр на экране больше не наш»: возврат из фона, закрытие
        // системного диалога. Отводим их в отдельный канал, чтобы UI-поток
        // перерисовался целиком.
        if matches!(event, Event::Repaint | Event::Show) {
            let _ = redraw_tx.send(());
        }

        // Обрыв канала означает, что UI-поток умер. Продолжать бессмысленно:
        // события уходили бы в никуда, а приложение висело бы с мёртвым
        // экраном, поэтому закрываемся штатно.
        if evt_tx.send(event).is_err() {
            unsafe {
                iv.CloseApp();
            }
        }

        Some(())
    });
}

/// Список книг постранично.
///
/// Прокрутки нет намеренно: на e-ink каждый кадр стоит полного обновления
/// экрана, поэтому список нарезается здесь, а Slint показывает ровно одну
/// готовую страницу. Сколько строк в неё влезает, считает сам Slint — от
/// фактической высоты окна, — и отдаёт через `rows-per-page`.
struct Pager {
    window: slint::Weak<MainWindow>,
    model: Rc<VecModel<BookItem>>,
    cmd_tx: Sender<Cmd>,

    all: RefCell<Vec<Book>>,
    covers: RefCell<HashMap<i64, Image>>,
    states: RefCell<HashMap<i64, &'static str>>,
    page: Cell<usize>,
    rows: Cell<usize>,
}

impl Pager {
    fn new(
        window: slint::Weak<MainWindow>,
        model: Rc<VecModel<BookItem>>,
        cmd_tx: Sender<Cmd>,
    ) -> Self {
        Self {
            window,
            model,
            cmd_tx,
            all: RefCell::new(Vec::new()),
            covers: RefCell::new(HashMap::new()),
            states: RefCell::new(HashMap::new()),
            page: Cell::new(0),
            rows: Cell::new(1),
        }
    }

    fn set_books(&self, books: Vec<Book>) {
        *self.all.borrow_mut() = books;
        self.covers.borrow_mut().clear();
        self.states.borrow_mut().clear();
        self.page.set(0);
        self.render();
    }

    /// Число строк на странице меняется вместе с размером окна, поэтому
    /// приходит не один раз при старте.
    fn set_rows(&self, rows: usize) {
        if self.rows.replace(rows.max(1)) != rows.max(1) {
            self.render();
        }
    }

    fn turn(&self, forward: bool) {
        let page = self.page.get();
        let next = if forward {
            page + 1
        } else {
            page.saturating_sub(1)
        };

        if next != page && next < self.total_pages() {
            self.page.set(next);
            self.render();
        }
    }

    fn set_state(&self, id: i64, state: &'static str) {
        self.states.borrow_mut().insert(id, state);
        self.render();
    }

    fn set_cover(&self, id: i64, image: Image) {
        self.covers.borrow_mut().insert(id, image);
        self.render();
    }

    fn total_pages(&self) -> usize {
        self.all.borrow().len().div_ceil(self.rows.get().max(1)).max(1)
    }

    fn render(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };

        let all = self.all.borrow();
        let rows = self.rows.get().max(1);
        let total = all.len().div_ceil(rows).max(1);

        // Страница могла оказаться за концом списка: сменились настройки,
        // повернули экран, пришёл более короткий список.
        let page = self.page.get().min(total - 1);
        self.page.set(page);

        let start = (page * rows).min(all.len());
        let end = (start + rows).min(all.len());
        let visible = &all[start..end];

        let covers = self.covers.borrow();
        let states = self.states.borrow();

        self.model.set_vec(
            visible
                .iter()
                .map(|book| BookItem {
                    id: book.id as i32,
                    title: book.title.clone().into(),
                    author: book.author.clone().into(),
                    format: book.format.clone().unwrap_or_else(|| "—".to_string()).into(),
                    state: states.get(&book.id).copied().unwrap_or_default().into(),
                    cover: covers.get(&book.id).cloned().unwrap_or_default(),
                })
                .collect::<Vec<_>>(),
        );

        window.set_page_label(if all.is_empty() {
            Default::default()
        } else {
            format!("стр. {} / {}", page + 1, total).into()
        });
        window.set_has_prev(page > 0);
        window.set_has_next(page + 1 < total);

        // Обложки грузим только для показанной страницы — это и есть главная
        // выгода пагинации перед списком на 200 строк.
        let missing: Vec<i64> = visible
            .iter()
            .map(|book| book.id)
            .filter(|id| !covers.contains_key(id))
            .collect();

        if !missing.is_empty() {
            let _ = self.cmd_tx.send(Cmd::Covers(missing));
        }
    }
}

fn ui_main(iv: &'static Inkview, evt_rx: Receiver<Event>, redraw_rx: Receiver<()>) {
    // До EVT_INIT трогать экран нельзя.
    loop {
        match evt_rx.recv() {
            Ok(Event::Init) => break,
            Ok(_) => continue,
            Err(_) => return,
        }
    }

    let screen = Screen::new(iv);
    // Физический dpi экрана — основа вёрстки: все размеры задаются в
    // миллиметрах и переводятся в логические пиксели с учётом dpi и
    // scale_factor. Для PB632 dpi=300; на всякий случай подстраховываемся от
    // нулевого значения, чтобы не делить интерфейс на ноль.
    let screen_dpi = screen.dpi().max(1) as f32;
    let backend = inkview_slint::Backend::new(screen, evt_rx);
    slint::platform::set_platform(Box::new(backend)).expect("платформа уже установлена");

    let (loaded, cfg_path, note) = Config::load();
    let window = MainWindow::new().expect("не удалось создать окно");

    // Настройки живут на UI-потоке: их показывает экран настроек, правит
    // клавиатура и сохраняет в файл. Рабочий поток получает копию.
    let cfg = Rc::new(RefCell::new(loaded.clone()));

    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let (answer_tx, answer_rx) = mpsc::channel::<keyboard::Answer>();

    keyboard::init(iv, answer_tx);

    // Идентификатор сборки виден на устройстве постоянно — чтобы не гадать,
    // какой билд запущен.
    window.set_build_id(concat!("build ", env!("BUILD_ID")).into());

    let books = Rc::new(VecModel::<BookItem>::default());
    window.set_books(ModelRc::from(books.clone()));
    window.set_status(
        note.unwrap_or_else(|| format!("Конфиг: {}", cfg_path.display()))
            .into(),
    );
    show_config(&window, &cfg.borrow());

    let pager = Rc::new(Pager::new(window.as_weak(), books, cmd_tx.clone()));

    std::thread::Builder::new()
        .name("worker".to_string())
        .spawn(move || worker(iv, loaded, cmd_rx, msg_tx))
        .expect("не удалось запустить рабочий поток");

    window.on_refresh({
        let cmd_tx = cmd_tx.clone();
        move || {
            let _ = cmd_tx.send(Cmd::Refresh);
        }
    });

    window.on_download({
        let cmd_tx = cmd_tx.clone();
        move |id| {
            let _ = cmd_tx.send(Cmd::Download(id));
        }
    });

    window.on_prev_page({
        let pager = pager.clone();
        move || pager.turn(false)
    });

    window.on_next_page({
        let pager = pager.clone();
        move || pager.turn(true)
    });

    window.on_toggle_settings({
        let weak = window.as_weak();
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            window.set_settings_open(!window.get_settings_open());
        }
    });

    window.on_edit_field({
        let cfg = cfg.clone();
        move |index| {
            let Some(field) = Field::from_index(index) else {
                return;
            };
            // Пароль всегда набирается заново: показывать его нечем и
            // подставлять в клавиатуру незачем.
            let initial = match field {
                Field::Password => String::new(),
                other => current_value(&cfg.borrow(), other),
            };
            keyboard::request(field, initial);
        }
    });

    // Бэкенд inkview-slint не реализует event loop proxy, поэтому
    // `invoke_from_event_loop` из чужого потока не сработает. Забираем
    // результаты таймером: его крутит штатный механизм Slint внутри цикла
    // бэкенда.
    let timer = slint::Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(250), {
        let weak = window.as_weak();
        let pager = pager.clone();
        let cfg = cfg.clone();
        let cmd_tx = cmd_tx.clone();
        let cfg_path = cfg_path.clone();

        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };

            // scale_factor бэкенд выставляет только после старта своего цикла,
            // поэтому «миллиметр в логических пикселях» пересчитываем здесь.
            // 1 мм = dpi/25.4 физических px, а логические = физические /
            // scale_factor. Отсюда вся вёрстка получает физически корректный
            // масштаб на любом экране.
            let scale = window.window().scale_factor();
            if scale > 0.0 {
                let mm = screen_dpi / 25.4 / scale;
                let metrics = window.global::<Metrics>();
                if (metrics.get_mm() - mm).abs() > 0.001 {
                    metrics.set_mm(mm);
                }
            }

            pager.set_rows(window.get_rows_per_page().max(1) as usize);

            while let Ok(msg) = msg_rx.try_recv() {
                apply(&window, &pager, msg);
            }

            while let Ok((field, value)) = answer_rx.try_recv() {
                apply_answer(&window, &cfg, &cfg_path, &cmd_tx, field, value);
            }

            if redraw_rx.try_iter().count() > 0 {
                window.set_repaint_tick(window.get_repaint_tick() + 1);
            }
        }
    });

    let _ = cmd_tx.send(Cmd::Refresh);

    window.run().expect("цикл событий завершился с ошибкой");
}

fn apply(window: &MainWindow, pager: &Rc<Pager>, msg: Msg) {
    match msg {
        Msg::Status(text) => window.set_status(text.into()),
        Msg::Busy(busy) => window.set_busy(busy),
        Msg::Books(list) => pager.set_books(list),
        Msg::BookState(id, state) => pager.set_state(id as i64, state),
        Msg::Cover {
            id,
            rgb,
            width,
            height,
        } => {
            let buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&rgb, width, height);
            pager.set_cover(id, Image::from_rgb8(buffer));
        }
    }
}

/// Показывает текущие настройки на экране настроек.
fn show_config(window: &MainWindow, cfg: &Config) {
    let or_dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".to_string());

    window.set_server(cfg.server.clone().into());
    window.set_cfg_server(cfg.server.clone().into());
    window.set_cfg_user(or_dash(&cfg.user).into());
    window.set_cfg_password(if cfg.password.is_some() { "••••••" } else { "—" }.into());
    window.set_cfg_library(or_dash(&cfg.library).into());
    window.set_cfg_download_dir(cfg.download_dir.display().to_string().into());
    window.set_cfg_formats(cfg.formats.join(", ").into());
    window.set_cfg_limit(cfg.limit.to_string().into());
}

fn current_value(cfg: &Config, field: Field) -> String {
    match field {
        Field::Server => cfg.server.clone(),
        Field::User => cfg.user.clone().unwrap_or_default(),
        Field::Password => cfg.password.clone().unwrap_or_default(),
        Field::Library => cfg.library.clone().unwrap_or_default(),
        Field::DownloadDir => cfg.download_dir.display().to_string(),
        Field::Formats => cfg.formats.join(","),
        Field::Limit => cfg.limit.to_string(),
    }
}

/// Принимает то, что набрали на клавиатуре: обновляет настройки, пишет их на
/// диск и пересобирает клиента в рабочем потоке.
fn apply_answer(
    window: &MainWindow,
    cfg: &Rc<RefCell<Config>>,
    cfg_path: &Path,
    cmd_tx: &Sender<Cmd>,
    field: Field,
    value: Option<String>,
) {
    // Клавиатуру всегда закрывают поверх нашего кадра, даже если ввод отменили,
    // поэтому перерисовываем в любом случае.
    window.set_repaint_tick(window.get_repaint_tick() + 1);

    let Some(value) = value else {
        return;
    };
    let value = value.trim().to_string();
    let optional = |v: String| if v.is_empty() { None } else { Some(v) };

    {
        let mut cfg = cfg.borrow_mut();
        match field {
            Field::Server => cfg.server = value.trim_end_matches('/').to_string(),
            Field::User => cfg.user = optional(value),
            Field::Password => cfg.password = optional(value),
            Field::Library => cfg.library = optional(value),
            Field::DownloadDir if !value.is_empty() => cfg.download_dir = PathBuf::from(value),
            Field::Formats if !value.is_empty() => {
                cfg.formats = value
                    .split(',')
                    .map(|f| f.trim().to_uppercase())
                    .filter(|f| !f.is_empty())
                    .collect();
            }
            Field::Limit => {
                if let Ok(n) = value.parse::<usize>() {
                    cfg.limit = n.clamp(1, 5000);
                }
            }
            // Пустое значение для пути или списка форматов — не изменение,
            // а очистка обязательного поля; молча оставляем прежнее.
            Field::DownloadDir | Field::Formats => {}
        }
    }

    let cfg = cfg.borrow();
    show_config(window, &cfg);

    match cfg.save(cfg_path) {
        Ok(()) => window.set_status("Настройки сохранены".into()),
        Err(e) => window.set_status(format!("Не удалось сохранить настройки: {e}").into()),
    }

    let _ = cmd_tx.send(Cmd::Reconfigure(cfg.clone()));
}

fn decode_cover(jpeg: &[u8]) -> Result<(Vec<u8>, u32, u32), calibre::Error> {
    let decoded = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)?;
    let rgb = decoded.to_rgb8();
    let (width, height) = rgb.dimensions();

    Ok((rgb.into_raw(), width, height))
}

fn worker(iv: &'static Inkview, cfg: Config, cmd_rx: Receiver<Cmd>, msg_tx: Sender<Msg>) {
    let mut cfg = cfg;
    let mut client = Client::new(&cfg);
    let mut known: Vec<Book> = Vec::new();

    while let Ok(cmd) = cmd_rx.recv() {
        // Обложки грузятся фоном и строку состояния не занимают: пользователь
        // в это время листает список, и «Подождите…» было бы враньём.
        let noisy = !matches!(cmd, Cmd::Covers(_));
        if noisy {
            let _ = msg_tx.send(Msg::Busy(true));
        }

        match cmd {
            // Клиент кэширует адрес, авторизацию и id библиотеки, поэтому при
            // смене настроек его проще пересоздать, чем править по частям.
            Cmd::Reconfigure(updated) => {
                cfg = updated;
                client = Client::new(&cfg);
                known.clear();
                let _ = msg_tx.send(Msg::Books(Vec::new()));
            }

            Cmd::Covers(ids) => {
                for id in ids {
                    let Ok(jpeg) = client.thumbnail(id, COVER_SIZE.0, COVER_SIZE.1) else {
                        continue;
                    };
                    // Книга без обложки — обычное дело, молча оставляем пустое
                    // место: ругаться в строке состояния тут не на что.
                    if let Ok((rgb, width, height)) = decode_cover(&jpeg) {
                        let _ = msg_tx.send(Msg::Cover {
                            id,
                            rgb,
                            width,
                            height,
                        });
                    }
                }
            }

            Cmd::Refresh => {
                let _ = msg_tx.send(Msg::Status("Проверяю сеть…".to_string()));

                match net::ensure_online(iv) {
                    Ok(()) => {
                        let _ = msg_tx.send(Msg::Status("Загружаю список книг…".to_string()));
                        match client.list() {
                            Ok(list) => {
                                let _ = msg_tx
                                    .send(Msg::Status(format!("Книг в списке: {}", list.len())));
                                known = list.clone();
                                let _ = msg_tx.send(Msg::Books(list));
                            }
                            Err(e) => {
                                let _ = msg_tx
                                    .send(Msg::Status(format!("Не удалось получить список: {e}")));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = msg_tx.send(Msg::Status(e));
                    }
                }
            }

            Cmd::Download(id) => match known.iter().find(|b| b.id as i32 == id).cloned() {
                None => {
                    let _ = msg_tx.send(Msg::Status(
                        "Книга не найдена, обновите список".to_string(),
                    ));
                }
                Some(book) if book.format.is_none() => {
                    let _ = msg_tx.send(Msg::BookState(id, "нет формата"));
                    let _ = msg_tx.send(Msg::Status(format!(
                        "«{}»: нет ни одного из нужных форматов",
                        book.title
                    )));
                }
                Some(book) => {
                    let _ = msg_tx.send(Msg::BookState(id, "…"));
                    let _ = msg_tx.send(Msg::Status(format!("Скачиваю «{}»…", book.title)));

                    let result = net::ensure_online(iv)
                        .map_err(calibre::Error::from)
                        .and_then(|()| client.download(&book, &cfg.download_dir));

                    match result {
                        Ok(path) => {
                            let _ = msg_tx.send(Msg::BookState(id, "готово"));
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let _ = msg_tx.send(Msg::Status(format!("Сохранено: {name}")));
                        }
                        Err(e) => {
                            let _ = msg_tx.send(Msg::BookState(id, "ошибка"));
                            let _ = msg_tx.send(Msg::Status(format!("Ошибка загрузки: {e}")));
                        }
                    }
                }
            },
        }

        if noisy {
            let _ = msg_tx.send(Msg::Busy(false));
        }
    }
}
