use inkview::bindings::Inkview;

/// Поднимает Wi-Fi, если он выключен.
///
/// `QueryNetwork` возвращает битовую маску состояния; ненулевой бит
/// `NET_CONNECTED` означает, что соединение уже есть. `NetConnect(NULL)`
/// подключается к последней использованной сети и сам показывает системный
/// диалог, если нужна ручная настройка.
pub fn ensure_online(iv: &Inkview) -> Result<(), String> {
    const NET_CONNECTED: i32 = 0x00000001;

    unsafe {
        if iv.QueryNetwork() & NET_CONNECTED != 0 {
            return Ok(());
        }

        let result = iv.NetConnect(std::ptr::null());
        if result == 0 {
            Ok(())
        } else {
            Err(format!("не удалось подключиться к сети (код {result})"))
        }
    }
}
