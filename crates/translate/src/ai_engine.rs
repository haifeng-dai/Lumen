use anyhow::{Result, anyhow};
use log::{debug, error, info};
use std::future::Future;
use std::pin::Pin;

use crate::TranslationBackend;

pub struct AiTranslateBackend {
    service: ai::AiService,
    target_lang_map: Vec<(String, String)>,
}

impl AiTranslateBackend {
    pub fn new(kind: ai::BackendKind, config: &ai::AiConfig) -> Self {
        info!(
            "AiTranslateBackend::new: kind={:?}, model={}, api_base={}",
            kind, config.model, config.api_base,
        );
        Self {
            service: ai::AiService::new(kind, config),
            target_lang_map: vec![
                ("zh".to_string(), "中文".to_string()),
                ("zh-CN".to_string(), "简体中文".to_string()),
                ("zh-TW".to_string(), "繁体中文".to_string()),
                ("en".to_string(), "英语".to_string()),
                ("ja".to_string(), "日语".to_string()),
                ("ko".to_string(), "韩语".to_string()),
                ("fr".to_string(), "法语".to_string()),
                ("de".to_string(), "德语".to_string()),
                ("es".to_string(), "西班牙语".to_string()),
                ("pt".to_string(), "葡萄牙语".to_string()),
                ("ru".to_string(), "俄语".to_string()),
                ("ar".to_string(), "阿拉伯语".to_string()),
            ],
        }
    }

    fn map_lang(&self, code: &str) -> String {
        let code = code.split('-').next().unwrap_or(code);
        self.target_lang_map
            .iter()
            .find(|(c, _)| c == code)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| {
                debug!(
                    "AiTranslateBackend: 未找到语言映射, code={}, 直接使用原始代码",
                    code
                );
                code.to_string()
            })
    }
}

impl TranslationBackend for AiTranslateBackend {
    fn translate(
        &self,
        text: &str,
        target_lang: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let service = self.service.clone();
        let text_len = text.len();
        let text = text.to_string();
        let target_lang = target_lang.to_string();
        let lang_name = self.map_lang(&target_lang);

        info!(
            "AiTranslateBackend::translate: 开始, backend={}, model={}, lang={}, lang_name={}, text_len={}",
            service.name(),
            service.model(),
            target_lang,
            lang_name,
            text_len,
        );

        Box::pin(async move {
            if text.is_empty() {
                debug!("AiTranslateBackend::translate: 文本为空，直接返回");
                return Ok(String::new());
            }

            debug!(
                "AiTranslateBackend::translate: 构造 prompt, target={lang_name}, text_preview={}",
                text,
            );

            let messages = vec![ai::ChatMessage::user(format!(
                "将以下学术文本翻译为{lang_name}，将其中的所有数学公式、希腊字母、变量与符号均转换为 Markdown LaTeX 格式，只返回翻译结果：\n\n{text}"
            ))];

            let system_prompt = "你是一个学术翻译助手。请将用户提供的学术文本翻译为指定语言，并严格遵守以下规则：\n\
1. 保持原意、严谨的学术风格和专业术语准确性；\n\
2. 【公式与符号转 Markdown/LaTeX】：\n\
   - 自动识别并转换所有希腊字母（如 alpha -> $\\alpha$、beta -> $\\beta$、theta -> $\\theta$、lambda -> $\\lambda$ 等）；\n\
   - 自动识别并转换所有数学变量、参数、带上下标表达式（如 x_i -> $x_i$、y^{t-1} -> $y^{t-1}$、f(x) -> $f(x)$）；\n\
   - 自动识别数学关系符与算子（如 in -> $\\in$、approx -> $\\approx$、sum -> $\\sum$）；\n\
   - 单个变量或希腊字母在正文中也必须使用行内公式 $...$ 包裹，独立公式使用 $$...$$ 包裹，严禁遗漏；\n\
3. 【输出格式】：使用标准 Markdown 格式输出；\n\
4. 【纯净输出】：只返回翻译正文，严禁添加任何前后缀或引导解释（如禁止输出'翻译如下：'等）。";

            debug!("AiTranslateBackend::translate: 调用 AiService::chat...");
            match service.chat(&messages, Some(system_prompt)).await {
                Ok(result) => {
                    let result = result.trim().to_string();
                    info!(
                        "AiTranslateBackend::translate: 成功, result_len={}, result_preview={}",
                        result.len(),
                        result,
                    );
                    Ok(result)
                }
                Err(e) => {
                    error!("AiTranslateBackend::translate: 翻译失败: {e:?}");
                    Err(anyhow!("AI 翻译失败: {e}"))
                }
            }
        })
    }
}
