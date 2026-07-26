use std::path::Path;

/// Пары «обычное начертание / жирное». Берётся первая пара, найденная целиком.
/// Все три семейства покрывают кириллицу — это и есть критерий отбора.
const FONT_CANDIDATES: &[(&str, &str)] = &[
    (
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    ),
    (
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
    ),
    (
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    ),
    (
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    ),
];

fn main() {
    // Идентификатор сборки — минуты с начала суток UTC на момент компиляции.
    // Показывается в статусбаре, чтобы на устройстве было видно, какой билд
    // запущен, и не путать версии при итерациях. Пересчитывается каждый раз,
    // потому что build.rs перезапускается при любой правке исходников.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs / 86400;
    let hhmm = (secs % 86400) / 60;
    println!(
        "cargo:rustc-env=BUILD_ID={}-{:02}{:02}",
        day % 1000,
        hhmm / 60,
        hhmm % 60
    );
    // Ссылка на несуществующий файл заставляет cargo считать build.rs всегда
    // устаревшим и перезапускать его каждую сборку — иначе BUILD_ID застынет
    // на значении с последнего запуска build.rs.
    println!("cargo:rerun-if-changed=.build-id-always-rerun");

    println!("cargo:rerun-if-env-changed=SLINT_DEFAULT_FONT");
    println!("cargo:rerun-if-env-changed=SLINT_FONT_PATH");

    // Шрифт выбираем на машине сборки и вшиваем в бинарник: на ридере
    // системные шрифты приложению недоступны.
    if std::env::var_os("SLINT_DEFAULT_FONT").is_none() {
        let found = FONT_CANDIDATES
            .iter()
            .find(|(regular, bold)| Path::new(regular).exists() && Path::new(bold).exists());

        match found {
            Some((regular, bold)) => {
                // SAFETY: build.rs однопоточен до вызова компилятора Slint,
                // который эти переменные и читает.
                unsafe {
                    std::env::set_var("SLINT_DEFAULT_FONT", regular);
                    std::env::set_var("SLINT_FONT_PATH", bold);
                }
            }
            None => panic!(
                "не найден шрифт с кириллицей; установите noto-fonts, ttf-liberation \
                 или ttf-dejavu, либо задайте SLINT_DEFAULT_FONT и SLINT_FONT_PATH вручную"
            ),
        }
    }

    // Софтварный рендерер без системных шрифтов: глифы должны уехать внутрь
    // бинарника. SDF (а не растровые) — потому что размеры шрифтов у нас
    // вычисляются из мм и dpi, а не заданы литералами в px. Растровое
    // встраивание берёт лишь литеральные размеры и весь текст рисовался бы
    // одним 12px-атласом; SDF рендерит любой размер из одного представления.
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer)
        .with_sdf_fonts(true);

    slint_build::compile_with_config("ui/app.slint", config).unwrap();
}
