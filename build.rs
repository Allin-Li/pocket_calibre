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
    // бинарника. Растровые (а не SDF) — на e-ink чёткость текста важнее
    // лишних сотен килобайт.
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);

    slint_build::compile_with_config("ui/app.slint", config).unwrap();
}
