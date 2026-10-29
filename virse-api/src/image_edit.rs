//! 图片指令编辑与局部重绘，复用现有上传及 V2 任务协议。
use crate::{
    AssetUploadFile, Client, Error, GenerationReference, MediaKind, OutputTarget, ReferenceSource,
    Result, SubmitTask,
};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const MAX_IMAGE_MASK_BYTES: usize = 50 * 1024 * 1024;

/// 现有蒙版上传接口返回的文件引用；不是资产 ID 或下载 URL。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadedImageMask {
    filepath: String,
}

impl UploadedImageMask {
    fn validate(&self) -> Result<()> {
        if self.filepath.is_empty()
            || self.filepath.len() > 2048
            || self.filepath.starts_with('/')
            || self.filepath.contains(':')
            || self.filepath.contains('\\')
            || self.filepath.chars().any(char::is_control)
            || self
                .filepath
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(Error::new(
                "INVALID_RESPONSE",
                "Invalid uploaded mask reference",
            ));
        }
        Ok(())
    }
}

impl Client {
    /// 单次上传，不提供幂等回执。调用方须记录结果，响应丢失时不得自动重传。
    /// PNG 黑色保留，白色编辑；不转换灰度/透明度，不创建资源目录项。
    pub async fn upload_image_mask(&self, file: AssetUploadFile) -> Result<UploadedImageMask> {
        file.validate()?;
        if file.bytes.len() > MAX_IMAGE_MASK_BYTES || !file.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Mask must be a PNG of at most 50 MiB",
            ));
        }
        let form = Form::new()
            .part(
                "user_images",
                Part::bytes(file.bytes)
                    .file_name("mask.png")
                    .mime_str("image/png")
                    .map_err(|_| Error::protocol())?,
            )
            .text("save_asset", "false");
        let result = self.multipart_request("upload_images", form, None).await?;
        let paths = result
            .get("image_filepaths")
            .and_then(Value::as_array)
            .ok_or_else(Error::protocol)?;
        if paths.len() != 1 {
            return Err(Error::protocol());
        }
        let mask = UploadedImageMask {
            filepath: paths[0].as_str().ok_or_else(Error::protocol)?.into(),
        };
        mask.validate()?;
        Ok(mask)
    }
}

impl SubmitTask {
    /// 主图固定为 $1，额外参考从 $2 开始。无 mask 为 quick edit，有 mask 为 inpaint。
    pub fn image_edit(
        model: &str,
        prompt: &str,
        source: ReferenceSource,
        additional_references: Vec<GenerationReference>,
        parameters: Option<Map<String, Value>>,
        mask: Option<&UploadedImageMask>,
        output: OutputTarget,
    ) -> Result<Self> {
        if matches!(&source, ReferenceSource::Text { .. })
            || matches!(&source, ReferenceSource::Asset {asset_type,..} if asset_type != "image")
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Image editing requires an image source",
            ));
        }
        let references = std::iter::once(GenerationReference { source })
            .chain(additional_references)
            .collect();
        let mut request = Self::media(
            MediaKind::Image,
            model,
            prompt,
            references,
            parameters,
            output,
        )?;
        request.operation = "image_quick_edit".into();
        match mask {
            Some(mask) => request.with_image_mask(mask),
            None => Ok(request),
        }
    }

    /// 将已准备的图片编辑请求转为局部重绘，不更换来源或输出目标。
    pub fn with_image_mask(mut self, mask: &UploadedImageMask) -> Result<Self> {
        if self.operation != "image_quick_edit" && self.operation != "image_inpaint" {
            return Err(Error::new(
                "INVALID_INPUT",
                "Only image edits accept an inpaint mask",
            ));
        }
        mask.validate()?;
        self.input
            .insert("mask_image_filepath".into(), mask.filepath.clone().into());
        self.operation = "image_inpaint".into();
        self.validate()?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    fn output() -> OutputTarget {
        OutputTarget {
            output_index: 0,
            element_id: Some(Uuid::nil()),
            create_element: None,
        }
    }
    #[test]
    fn rejects_non_image_source_and_empty_prompt() {
        for source in [
            ReferenceSource::Text {
                value: "text".into(),
            },
            ReferenceSource::Asset {
                asset_type: "video".into(),
                asset_id: Uuid::nil(),
            },
        ] {
            assert!(
                SubmitTask::image_edit("model", "edit", source, vec![], None, None, output())
                    .is_err()
            );
        }
        assert!(
            SubmitTask::image_edit(
                "model",
                " ",
                ReferenceSource::Asset {
                    asset_type: "image".into(),
                    asset_id: Uuid::nil()
                },
                vec![],
                None,
                None,
                output()
            )
            .is_err()
        );
    }
    #[test]
    fn uploaded_mask_is_not_an_arbitrary_url_or_filesystem_path() {
        for filepath in [
            "",
            "https://example.com/mask.png",
            "/etc/passwd",
            "../mask.png",
            "a/../mask.png",
            "gs://bucket/mask.png",
        ] {
            assert!(
                UploadedImageMask {
                    filepath: filepath.into()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            UploadedImageMask {
                filepath: "uploads/random-mask.png".into()
            }
            .validate()
            .is_ok()
        );
    }
}
