//! Недостающие в glibc 2.23 функции libm.
//!
//! `tiny-skia` (приезжает как resvg → SVG-поддержка i-slint-core, а её включает
//! фича `std`) использует `f64::minimum`/`f64::maximum`. Rust лоуерит их в
//! вызовы libm-функций C23 `fminimum_num`/`fmaximum_num`, которые появились
//! только в glibc 2.35 — на PocketBook 632 их нет, и линковка падает.
//!
//! Реализация повторяет minimumNumber/maximumNumber из IEEE 754-2019:
//! NaN игнорируется, если второй операнд — число; нули различаются по знаку.
//! Обычные сравнения в такие же libm-вызовы не разворачиваются, рекурсии нет.
#![cfg(all(target_arch = "arm", target_os = "linux"))]

#[unsafe(no_mangle)]
pub extern "C" fn fminimum_num(x: f64, y: f64) -> f64 {
    if x.is_nan() {
        y
    } else if y.is_nan() {
        x
    } else if x < y {
        x
    } else if y < x {
        y
    } else if x.is_sign_negative() {
        // Операнды равны — остались только -0.0 и +0.0.
        x
    } else {
        y
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fminimum_numf(x: f32, y: f32) -> f32 {
    if x.is_nan() {
        y
    } else if y.is_nan() {
        x
    } else if x < y {
        x
    } else if y < x {
        y
    } else if x.is_sign_negative() {
        x
    } else {
        y
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fmaximum_num(x: f64, y: f64) -> f64 {
    if x.is_nan() {
        y
    } else if y.is_nan() {
        x
    } else if x > y {
        x
    } else if y > x {
        y
    } else if x.is_sign_positive() {
        x
    } else {
        y
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fmaximum_numf(x: f32, y: f32) -> f32 {
    if x.is_nan() {
        y
    } else if y.is_nan() {
        x
    } else if x > y {
        x
    } else if y > x {
        y
    } else if x.is_sign_positive() {
        x
    } else {
        y
    }
}
