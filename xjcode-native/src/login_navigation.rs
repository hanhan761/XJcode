//! Campus navigation policy. Never send credentials over plain HTTP.
pub fn target(raw: &str) -> Result<Option<String>, ()> {
    let mut url = reqwest::Url::parse(raw).map_err(|_| ())?;
    let campus = url
        .host_str()
        .is_some_and(|host| host == "xjtu.edu.cn" || host.ends_with(".xjtu.edu.cn"));
    if !campus || !url.username().is_empty() || url.password().is_some() {
        return Err(());
    }
    if url.scheme() == "https" {
        return Ok(None);
    }
    if url.scheme() == "http"
        && matches!(url.host_str(), Some("org.xjtu.edu.cn" | "ai.xjtu.edu.cn"))
        && url.port().is_none()
    {
        url.set_scheme("https").map_err(|_| ())?;
        return Ok(Some(url.to_string()));
    }
    Err(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrades_authorized_portal_callback() {
        assert_eq!(
            target("http://ai.xjtu.edu.cn/api/auth/callback?code=fixture&state=fixture").unwrap(),
            Some("https://ai.xjtu.edu.cn/api/auth/callback?code=fixture&state=fixture".into())
        );
    }
    #[test]
    fn upgrades_real_campus_sso_hop_without_losing_parameters() {
        let next = target("http://org.xjtu.edu.cn/openplatform/g/admin/oauth/authorize?appId=fixture&redirectUri=https%3A%2F%2Fai.xjtu.edu.cn%2Fcallback").unwrap().unwrap();
        assert_eq!(
            next,
            "https://org.xjtu.edu.cn/openplatform/g/admin/oauth/authorize?appId=fixture&redirectUri=https%3A%2F%2Fai.xjtu.edu.cn%2Fcallback"
        );
    }
    #[test]
    fn keeps_encrypted_campus_navigation() {
        assert_eq!(target("https://org.xjtu.edu.cn/login"), Ok(None));
    }
    #[test]
    fn refuses_external_and_unsafe_schemes() {
        for url in [
            "https://xjtu.edu.cn.evil.test/",
            "http://evil.test/",
            "javascript:alert(1)",
            "file:///C:/secret",
        ] {
            assert!(target(url).is_err());
        }
    }
}
