//! Account-scoped, read-only model catalog checks; no task content or credentials are saved.
use crate::{api::{Model, TopClient}, auth::{Credentials, atomic_write}, setup::Settings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes { pub added: usize, pub removed: usize, pub altered: usize }
impl Changes { fn any(self) -> bool { self.added + self.removed + self.altered > 0 } }
fn signature(m: &Model) -> (&str, &str, &str, Vec<&str>) {
    let mut features = m.features.iter().map(String::as_str).collect::<Vec<_>>();
    features.sort_unstable(); features.dedup();
    (&m.service_id, &m.upstream_name, &m.source_type, features)
}
pub fn compare(old: &[Model], new: &[Model]) -> Changes {
    let a = old.iter().map(|m| (m.name.as_str(), m)).collect::<BTreeMap<_,_>>();
    let b = new.iter().map(|m| (m.name.as_str(), m)).collect::<BTreeMap<_,_>>();
    Changes {
        added: b.keys().filter(|k| !a.contains_key(*k)).count(),
        removed: a.keys().filter(|k| !b.contains_key(*k)).count(),
        altered: a.iter().filter(|(k,v)| b.get(*k).is_some_and(|other| signature(v) != signature(other))).count(),
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Cache { scope: String, checked: i64, failed: bool, changes: Changes, #[serde(default)] selected_missing: bool }
fn scope(s: &Settings) -> String {
    let mut h=Sha256::new();
    for v in [s.account_id.as_str(), s.workspace_id.as_str(), s.model.as_str(), crate::api::ENDPOINT] {
        h.update((v.len() as u64).to_be_bytes()); h.update(v.as_bytes());
    }
    hex::encode(h.finalize())
}
fn is_fresh(c: &Cache, now: i64) -> bool {
    let interval = if c.failed {1800} else {86400};
    c.checked <= now && now.saturating_sub(c.checked) < interval
}
fn notice(c: &Cache) -> Option<String> {
    if c.failed && c.selected_missing {
        return Some("上次检查发现所选模型不在授权目录，本次检查未能确认；未自动切换，请检查网络并运行 xjcode models --refresh --select。".into());
    }
    if c.failed && c.changes.any() {
        return Some("上次检查发现模型目录有变化，本次检查未能确认；未自动切换，请检查网络后运行 xjcode models --refresh。".into());
    }
    if c.selected_missing {
        Some("当前模型不在最新授权目录；未自动切换。请运行 xjcode models --refresh --select 重新选择。".into())
    } else if c.changes.any() {
        Some(format!("交晓智模型目录有变化（新增 {}、移除 {}、配置变更 {}）；运行 xjcode models --refresh 查看并更新，当前模型不会自动切换。",c.changes.added,c.changes.removed,c.changes.altered))
    } else if c.failed { Some("交晓智模型目录暂未确认，继续使用本地目录；这不代表模型上游不可用。".into()) } else { None }
}
fn store(home: &Path, c: &Cache) { if let Ok(bytes)=serde_json::to_vec(c) { let _=atomic_write(&home.join("catalog-watch.json"), &bytes); } }
pub fn remember(home: &Path, settings: &Settings) {
    store(home,&Cache {scope:scope(settings),checked:chrono::Utc::now().timestamp(),failed:false,changes:Changes::default(),selected_missing:false});
}
pub async fn check(home: &Path, credentials: Credentials, settings: &Settings) -> Option<String> {
    let key=scope(settings);let now=chrono::Utc::now().timestamp();
    let previous=std::fs::read(home.join("catalog-watch.json")).ok().filter(|b|b.len()<8192)
      .and_then(|b|serde_json::from_slice::<Cache>(&b).ok()).filter(|c|c.scope==key);
    if let Some(c)=previous.as_ref() {
        if is_fresh(c,now) {return notice(c)}
    }
    let result=match TopClient::new(credentials) {
        Ok(client)=>tokio::time::timeout(Duration::from_secs(4),client.models(&settings.workspace_id)).await.ok().and_then(Result::ok).filter(|m|!m.is_empty()),
        Err(_)=>None,
    };
    let c=match result {
        Some(models)=>Cache {scope:key,checked:now,failed:false,changes:compare(&settings.models,&models),selected_missing:!models.iter().any(|m|m.name==settings.model)},
        None=>{let (changes,missing)=previous.map(|v|(v.changes,v.selected_missing)).unwrap_or_default();Cache {scope:key,checked:now,failed:true,changes,selected_missing:missing}},
    };
    store(home,&c);notice(&c)
}
#[cfg(test)] mod tests {
 use super::*;
 fn m(name:&str,id:&str,features:&[&str])->Model { Model {name:name.into(),service_id:id.into(),upstream_name:name.into(),source_type:"external".into(),features:features.iter().map(|v|v.to_string()).collect(),verified_image_input:false} }
 fn settings()->Settings {Settings {account_id:"account".into(),workspace_id:"workspace".into(),workspace_name:"w".into(),model:"a".into(),models:vec![m("a","1",&[])]}}
 #[test] fn detects_additions_removals_and_route_changes_without_local_capability_noise() {
  let old=vec![m("a","1",&["vision","tools"]),m("gone","2",&[])];
  let mut unchanged=m("a","1",&["tools","vision"]); unchanged.verified_image_input=true;
  assert_eq!(compare(&old,&[unchanged.clone(),m("new","3",&[])]),Changes{added:1,removed:1,altered:0});
  unchanged.service_id="different".into();assert_eq!(compare(&old,&[unchanged]).altered,1);
 }
 #[test] fn failure_retries_early_and_future_timestamps_are_stale() {
  let mut c=Cache{scope:"x".into(),checked:1000,failed:true,changes:Changes::default(),selected_missing:false};
  assert!(is_fresh(&c, 2799));assert!(!is_fresh(&c,2800));assert!(!is_fresh(&c,999));
  c.failed=false;assert!(is_fresh(&c,87399));assert!(!is_fresh(&c,87400));
 }
 #[tokio::test] async fn cached_change_is_reported_until_refresh_and_never_calls_network() {
  let dir=tempfile::tempdir().unwrap();let s=settings();
  store(dir.path(),&Cache{scope:scope(&s),checked:chrono::Utc::now().timestamp(),failed:false,changes:Changes{added:1,removed:0,altered:0},selected_missing:false});
  let credentials=Credentials{access_key:"fixture-access".into(),secret:"fixture-secret".into(),account_id:s.account_id.clone()};
  let warning=check(dir.path(),credentials.clone(),&s).await.unwrap();assert!(warning.contains("新增 1"));
  remember(dir.path(),&s);assert!(check(dir.path(),credentials,&s).await.is_none());
  let stored=std::fs::read_to_string(dir.path().join("catalog-watch.json")).unwrap();assert!(!stored.contains("fixture-secret"));assert!(!stored.contains("account"));
 }
 #[test] fn cached_notices_are_scoped_and_contain_no_ids() {
  let s=settings();let a=scope(&s);let mut t=s.clone();t.workspace_id="other".into();assert_ne!(a,scope(&t));t=s.clone();t.model="different".into();assert_ne!(a,scope(&t));
  let c=Cache{scope:a,checked:1,failed:false,changes:Changes{added:1,removed:0,altered:0},selected_missing:false};
  let n=notice(&c).unwrap();assert!(n.contains("--refresh"));assert!(!n.contains("account"));
  let mut missing=Cache {selected_missing:true,..c};assert!(notice(&missing).unwrap().contains("--refresh --select"));missing.failed=true;assert!(notice(&missing).unwrap().contains("上次检查"));assert!(notice(&missing).unwrap().contains("本次检查未能确认"));
 }
}
