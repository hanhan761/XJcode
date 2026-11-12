//! IAM lists team spaces. The portal's personal space is scoped to the verified
//! account and is authorized separately by the model gateway. Never create it here.
use crate::api::Workspace;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

pub fn total(value: &Value, label: &str) -> Result<Option<usize>> {
    match value.get("Total") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(Some(
            n.as_u64().and_then(|n| usize::try_from(n).ok()).with_context(|| format!("{label}的 Total 字段无效"))?,
        )),
        Some(Value::String(s)) if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) => {
            Ok(Some(s.parse().with_context(|| format!("{label}的 Total 字段无效"))?))
        }
        _ => bail!("{label}的 Total 字段无效"),
    }
}

pub fn items<'a>(value: &'a Value, label: &str) -> Result<&'a [Value]> {
    ensure!(value.is_object(), "{label}格式无法识别，请更新 XJCode 后重试");
    let count = total(value, label)?;
    match value.get("Items") {
        Some(Value::Array(items)) => {
            ensure!(count.is_none_or(|n| n >= items.len()), "{label}的数量与条目不一致");
            Ok(items)
        }
        None | Some(Value::Null) if count == Some(0) => Ok(&[]),
        _ => bail!("{label}的 Items 字段无效；此错误来自校园接口，不是本地文件夹路径"),
    }
}

pub fn include_personal(mut teams: Vec<Workspace>, verified_account: &str) -> Result<Vec<Workspace>> {
    ensure!(!verified_account.is_empty(), "缺少已校验的校园账号，未推导个人空间");
    // Verified portal convention (2026-10-20). This identifies only the same
    // account's personal space; it does not grant access or initialize resources.
    let id = format!("personal-{verified_account}");
    if !teams.iter().any(|w| w.id == id) {
        teams.insert(0, Workspace { id, name: "个人空间".into() });
    }
    Ok(teams)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_team_catalog_does_not_hide_personal_space() {
        for data in [json!({"Items":null,"Total":0}), json!({"Items":[],"Total":"0"}), json!({"Total":0})] {
            assert!(items(&data, "工作区目录").unwrap().is_empty());
            let spaces = include_personal(vec![], "verified-account").unwrap();
            assert_eq!(spaces.len(), 1);
            assert_eq!(spaces[0].id, "personal-verified-account");
        }
    }

    #[test]
    fn personal_is_account_scoped_and_existing_team_choices_survive() {
        let teams = vec![Workspace {id:"team".into(),name:"团队".into()}];
        let spaces=include_personal(teams,"account-b").unwrap();
        assert_eq!(spaces[0].id,"personal-account-b");
        assert_eq!(spaces[1].id,"team");
        assert!(include_personal(vec![], "").is_err());
        let spaces=include_personal(spaces,"account-b").unwrap();
        assert_eq!(spaces.len(),2);
    }

    #[test]
    fn malformed_or_ambiguous_catalog_is_not_treated_as_empty() {
        for data in [json!(null),json!({"Items":null}),json!({"Items":null,"Total":1}),json!({"Items":{},"Total":0}),json!({"Items":[],"Total":false}),json!({"Items":[],"Total":-1}),json!({"Items":[],"Total":""}),json!({"Items":[{}],"Total":0})] {
            assert!(items(&data,"工作区目录").is_err());
        }
        assert_eq!(items(&json!({"Items":[{"ID":"team"}],"Total":"1"}),"工作区目录").unwrap().len(),1);
    }
}
