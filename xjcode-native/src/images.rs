//! Model-facing image attachments. Image bytes never become prompt text or diagnostics.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde_json::Value;

pub const MAX_IMAGES: usize = 8;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct InputImage {
    data_url: String,
    mime: &'static str,
    size: usize,
}

impl InputImage {
    pub fn from_data_url(url: &str) -> Result<Self> {
        ensure!(
            url.len() <= MAX_IMAGE_BYTES.div_ceil(3) * 4 + 64,
            "图片超过单张 8 MiB 限制，未发送"
        );
        let (header, encoded) = url
            .split_once(',')
            .context("图片须为本地附件的 data URL；不读取远程地址或把路径当作图片")?;
        let (mime, signature): (&str, &[u8]) = match header {
            "data:image/png;base64" => ("image/png", b"\x89PNG\r\n\x1a\n"),
            "data:image/jpeg;base64" => ("image/jpeg", b"\xff\xd8\xff"),
            "data:image/webp;base64" => ("image/webp", b"RIFF"),
            "data:image/gif;base64" => ("image/gif", b"GIF8"),
            _ => anyhow::bail!("不支持的图片附件格式；请使用 PNG、JPEG、WebP 或 GIF"),
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| anyhow::anyhow!("图片附件编码无效，未发送"))?;
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_IMAGE_BYTES,
            "图片为空或超过单张 8 MiB 限制，未发送"
        );
        ensure!(
            bytes.starts_with(signature),
            "图片内容与附件格式不符，未发送"
        );
        if mime == "image/webp" {
            ensure!(
                bytes.get(8..12) == Some(b"WEBP"),
                "WebP 图片附件无效，未发送"
            );
        }
        Ok(Self {
            data_url: url.to_owned(),
            mime,
            size: bytes.len(),
        })
    }

    pub fn data_url(&self) -> &str {
        &self.data_url
    }
    pub fn size(&self) -> usize {
        self.size
    }
    pub fn extension(&self) -> &'static str {
        match self.mime {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => "png",
        }
    }
}

pub fn validate(images: &[InputImage]) -> Result<()> {
    ensure!(
        images.len() <= MAX_IMAGES,
        "图片附件最多 8 张；请分批发送，未发起模型请求"
    );
    ensure!(
        images.iter().map(InputImage::size).sum::<usize>() <= MAX_TOTAL_IMAGE_BYTES,
        "图片附件总大小超过 16 MiB；请缩小图片或分批发送，未发起模型请求"
    );
    Ok(())
}

/// Only process typed image content in messages/tool results, never tool
/// arguments, user text that happens to contain JSON, schemas, or arbitrary URLs.
pub fn extract(history: &Value) -> Result<(Value, Vec<InputImage>)> {
    let mut preview = history.clone();
    let mut images = Vec::new();
    if let Some(items) = preview.as_array_mut() {
        for item in items {
            let field = match item["type"].as_str() {
                Some("message") => "content",
                Some("function_call_output" | "custom_tool_call_output") => "output",
                _ => continue,
            };
            let Some(parts) = item.get_mut(field).and_then(Value::as_array_mut) else {
                continue;
            };
            for part in parts {
                if part["type"] != "input_image" {
                    continue;
                }
                ensure!(
                    images.len() < MAX_IMAGES,
                    "图片附件最多 8 张；请压缩旧会话或分批发送"
                );
                let url = part["image_url"]
                    .as_str()
                    .context("图片附件缺少有效内容，未发送")?;
                images.push(InputImage::from_data_url(url)?);
                part["image_url"] = Value::String(format!("xjcode-image:{}", images.len()));
            }
        }
    }
    validate(&images)?;
    Ok((preview, images))
}

#[cfg(test)]
pub(crate) fn test_image_url() -> String {
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aD3sAAAAASUVORK5CYII=".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extraction_keeps_image_order_and_original_history_without_reading_paths() {
        let url = test_image_url();
        let history = json!([
            {"type":"message","role":"user","content":[{"type":"input_text","text":"inspect both images"},{"type":"input_image","image_url":url}]},
            {"type":"function_call","call_id":"call","name":"custom","arguments":{"type":"input_image","image_url":"ordinary tool argument"}},
            {"type":"function_call_output","call_id":"call","output":[{"type":"input_image","image_url":url,"detail":"high"}]}
        ]);
        let before = history.clone();
        let (preview, images) = extract(&history).unwrap();
        assert_eq!(history, before);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].data_url(), url);
        assert_eq!(images[1].data_url(), url);
        assert_eq!(preview[0]["content"][1]["image_url"], "xjcode-image:1");
        assert_eq!(preview[2]["output"][0]["image_url"], "xjcode-image:2");
        assert_eq!(preview[2]["output"][0]["detail"], "high");
        assert_eq!(preview[1], history[1]);
    }

    #[test]
    fn bad_images_and_remote_urls_are_rejected_without_echoing_content() {
        for url in [
            "https://private.example/image.png",
            "file:///C:/private.png",
            "data:image/png;base64,private-secret",
            "data:image/png;base64,",
            "data:image/png;base64,aGVsbG8=",
            "data:text/html;base64,private-secret",
        ] {
            let error = InputImage::from_data_url(url)
                .err()
                .expect("must reject")
                .to_string();
            assert!(!error.contains("private-secret"));
            assert!(!error.contains("private.example"));
        }
    }

    #[test]
    fn attachment_count_is_bounded_without_silently_dropping_images() {
        let url = test_image_url();
        let parts: Vec<_> = (0..=MAX_IMAGES)
            .map(|_| json!({"type":"input_image","image_url":url}))
            .collect();
        let history = json!([{"type":"message","role":"user","content":parts}]);
        assert!(extract(&history).is_err());
        let mut at_limit = history.clone();
        at_limit[0]["content"].as_array_mut().unwrap().pop();
        assert_eq!(extract(&at_limit).unwrap().1.len(), MAX_IMAGES);
    }

    #[test]
    fn aggregate_bytes_are_bounded_independently_of_text_budget() {
        let mut bytes = vec![0; MAX_IMAGE_BYTES];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let image = InputImage::from_data_url(&url).unwrap();
        assert!(validate(&[image.clone(), image.clone()]).is_ok());
        assert!(validate(&[image.clone(), image.clone(), image]).is_err());
    }
}
