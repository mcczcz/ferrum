//! LegacyMoney 0.23 的经济操作。服务缺失或停用时返回 `ErrorKind::ServiceUnavailable`。
//! 所有操作使用玩家的 XUID，在持有主线程令牌时调用。
use crate::{Error, MainThreadToken, Result, host};

/// 读取账户余额。
pub fn get(_token: &MainThreadToken<'_>, xuid: &str) -> Result<i64> {
    let mut balance = 0;
    host::check((host::llmoney_api()?.get()?)(xuid.into(), &mut balance))?;
    Ok(balance)
}

/// 设置账户余额。
pub fn set(_token: &MainThreadToken<'_>, xuid: &str, amount: i64) -> Result<()> {
    host::check((host::llmoney_api()?.set()?)(xuid.into(), amount))
}

/// 增加账户余额。
pub fn add(_token: &MainThreadToken<'_>, xuid: &str, amount: i64) -> Result<()> {
    host::check((host::llmoney_api()?.add()?)(xuid.into(), amount))
}

/// 扣减账户余额。
pub fn reduce(_token: &MainThreadToken<'_>, xuid: &str, amount: i64) -> Result<()> {
    host::check((host::llmoney_api()?.reduce()?)(xuid.into(), amount))
}

/// 在两个账户之间转账，备注可传空字符串。
pub fn transfer(
    _token: &MainThreadToken<'_>,
    from: &str,
    to: &str,
    amount: i64,
    note: &str,
) -> Result<()> {
    host::check((host::llmoney_api()?.transfer()?)(
        from.into(),
        to.into(),
        amount,
        note.into(),
    ))
}

/// 读取账户在给定秒数内的交易记录，返回 LegacyMoney 的原始文本。
pub fn history(_token: &MainThreadToken<'_>, xuid: &str, seconds: i32) -> Result<String> {
    let history = host::llmoney_api()?.history()?;
    host::read_string(|writer| history(xuid.into(), seconds, writer))
}

/// 清理全服交易记录。秒数直接传给 LegacyMoney 的 `LLMoney_ClearHist`。
pub fn clear_history(_token: &MainThreadToken<'_>, seconds: i32) -> Result<()> {
    host::check((host::llmoney_api()?.clear_history()?)(seconds))
}

/// 返回 LegacyMoney 排行榜中的 (XUID, 余额)，保留原始顺序。
pub fn ranking(_token: &MainThreadToken<'_>, limit: u16) -> Result<Vec<(String, i64)>> {
    let ranking = host::llmoney_api()?.ranking()?;
    let bytes =
        host::read_bytes(|writer| ranking(limit, writer)).map_err(host::error_from_status)?;
    let mut rows = Vec::new();
    let mut remaining = bytes.as_slice();
    let invalid = || Error::msg("宿主返回的 LLMoney 排行榜格式错误");
    while !remaining.is_empty() {
        let (balance, tail) = remaining.split_first_chunk::<8>().ok_or_else(invalid)?;
        let (length, tail) = tail.split_first_chunk::<8>().ok_or_else(invalid)?;
        let length = usize::try_from(u64::from_le_bytes(*length)).map_err(|_| invalid())?;
        let (xuid, tail) = tail.split_at_checked(length).ok_or_else(invalid)?;
        rows.push((
            String::from_utf8(xuid.to_vec())?,
            i64::from_le_bytes(*balance),
        ));
        remaining = tail;
    }
    Ok(rows)
}
