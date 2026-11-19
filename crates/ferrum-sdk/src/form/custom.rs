use std::collections::HashMap;
use std::future::Future;

use serde_json::{Value, json};

use crate::{MainThreadToken, ModContext, PlayerRef, Result};

use super::{FormOutcome, index, invalid, send, send_async};

/// 自定义表单，提交后按字段名读取结果。
pub struct CustomForm {
    title: String,
    elements: Vec<Element>,
}

struct Element {
    field: Option<(String, Option<Field>)>,
    json: Value,
}

impl CustomForm {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            elements: Vec::new(),
        }
    }

    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.elements.push(Element {
            field: None,
            json: json!({"type": "label", "text": text.into()}),
        });
        self
    }

    pub fn input(
        self,
        name: impl Into<String>,
        text: impl Into<String>,
        placeholder: impl Into<String>,
        default: impl Into<String>,
    ) -> Self {
        self.field(name, Some(Field::Text), json!({
            "type": "input", "text": text.into(), "placeholder": placeholder.into(), "default": default.into()
        }))
    }

    pub fn toggle(self, name: impl Into<String>, text: impl Into<String>, default: bool) -> Self {
        self.field(
            name,
            Some(Field::Toggle),
            json!({
                "type": "toggle", "text": text.into(), "default": default
            }),
        )
    }

    pub fn dropdown(
        self,
        name: impl Into<String>,
        text: impl Into<String>,
        options: impl IntoIterator<Item = impl Into<String>>,
        default: usize,
    ) -> Self {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        let field = (default < options.len()).then_some(Field::Choice(options.len()));
        self.field(
            name,
            field,
            json!({
                "type": "dropdown", "text": text.into(), "options": options, "default": default
            }),
        )
    }

    pub fn slider(
        self,
        name: impl Into<String>,
        text: impl Into<String>,
        min: f64,
        max: f64,
        step: f64,
        default: f64,
    ) -> Self {
        let field = (min.is_finite()
            && max.is_finite()
            && step.is_finite()
            && step >= 0.0
            && (min..=max).contains(&default))
        .then_some(Field::Slider { min, max });
        self.field(name, field, json!({
            "type": "slider", "text": text.into(), "min": min, "max": max, "step": step, "default": default
        }))
    }

    pub fn step_slider(
        self,
        name: impl Into<String>,
        text: impl Into<String>,
        steps: impl IntoIterator<Item = impl Into<String>>,
        default: usize,
    ) -> Self {
        let steps: Vec<String> = steps.into_iter().map(Into::into).collect();
        let field = (default < steps.len()).then_some(Field::Choice(steps.len()));
        self.field(
            name,
            field,
            json!({
                "type": "step_slider", "text": text.into(), "steps": steps, "default": default
            }),
        )
    }

    pub fn send(
        self,
        ctx: &ModContext,
        player: PlayerRef<'_>,
        callback: impl for<'tick> FnOnce(
            PlayerRef<'tick>,
            FormOutcome<CustomFormResponse>,
            &'tick MainThreadToken<'tick>,
        ) -> Result<()>
        + 'static,
    ) -> Result<()> {
        let (form, decode) = self.prepare()?;
        send(ctx, player, form, decode, callback)
    }

    /// 立即发送，返回可跨线程等待的响应。玩家断线或模组停用时返回错误。
    pub fn send_async(
        self,
        ctx: &ModContext,
        player: PlayerRef<'_>,
    ) -> Result<
        impl Future<Output = Result<FormOutcome<CustomFormResponse>>> + Send + 'static + use<>,
    > {
        let (form, decode) = self.prepare()?;
        send_async(ctx, player, form, decode)
    }

    fn field(mut self, name: impl Into<String>, field: Option<Field>, json: Value) -> Self {
        self.elements.push(Element {
            field: Some((name.into(), field)),
            json,
        });
        self
    }

    fn prepare(self) -> Result<(Value, impl FnOnce(Value) -> Result<CustomFormResponse>)> {
        let mut fields = HashMap::new();
        let mut content = Vec::with_capacity(self.elements.len());
        for element in self.elements {
            if let Some((name, field)) = element.field {
                let field =
                    field.ok_or_else(|| invalid(format!("表单字段 {name} 的默认值或范围无效")))?;
                if fields
                    .insert(name.clone(), (content.len(), field))
                    .is_some()
                {
                    return Err(invalid(format!("重复的表单字段名 {name}")));
                }
            }
            content.push(element.json);
        }
        let count = content.len();
        let form = json!({"type": "custom_form", "title": self.title, "content": content});
        Ok((form, move |value| {
            let Value::Array(mut values) = value else {
                return Err(invalid("自定义表单响应必须是数组"));
            };
            if values.len() != count {
                return Err(invalid("自定义表单响应的元素数量不匹配"));
            }
            let mut answers = HashMap::with_capacity(fields.len());
            for (name, (slot, field)) in fields {
                let value = values[slot].take();
                let answer = field
                    .read(value)
                    .ok_or_else(|| invalid(format!("表单字段 {name} 的响应无效")))?;
                answers.insert(name, answer);
            }
            Ok(CustomFormResponse { answers })
        }))
    }
}

#[derive(Clone, Copy)]
enum Field {
    Text,
    Toggle,
    Choice(usize),
    Slider { min: f64, max: f64 },
}

impl Field {
    fn read(self, value: Value) -> Option<Answer> {
        match (self, value) {
            (Self::Text, Value::String(text)) => Some(Answer::Text(text)),
            (Self::Toggle, Value::Bool(toggle)) => Some(Answer::Toggle(toggle)),
            (Self::Choice(count), value) => {
                index(&value).filter(|&i| i < count).map(Answer::Choice)
            }
            (Self::Slider { min, max }, value) => value
                .as_f64()
                .filter(|number| (min..=max).contains(number))
                .map(Answer::Slider),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Answer {
    Text(String),
    Toggle(bool),
    Choice(usize),
    Slider(f64),
}

/// 一次提交的输入值。字段缺失或读取类型不匹配时返回错误。
#[derive(Debug, Clone, PartialEq)]
pub struct CustomFormResponse {
    answers: HashMap<String, Answer>,
}

impl CustomFormResponse {
    pub fn text(&self, name: &str) -> Result<&str> {
        match self.answers.get(name) {
            Some(Answer::Text(text)) => Ok(text),
            _ => Err(invalid(format!("字段 {name} 缺失或类型不是文本"))),
        }
    }

    pub fn toggle(&self, name: &str) -> Result<bool> {
        match self.answers.get(name) {
            Some(Answer::Toggle(value)) => Ok(*value),
            _ => Err(invalid(format!("字段 {name} 缺失或类型不是开关"))),
        }
    }

    /// 下拉框或步进滑块中从 0 开始的选项索引。
    pub fn choice(&self, name: &str) -> Result<usize> {
        match self.answers.get(name) {
            Some(Answer::Choice(index)) => Ok(*index),
            _ => Err(invalid(format!("字段 {name} 缺失或类型不是选项"))),
        }
    }

    pub fn slider(&self, name: &str) -> Result<f64> {
        match self.answers.get(name) {
            Some(Answer::Slider(number)) => Ok(*number),
            _ => Err(invalid(format!("字段 {name} 缺失或类型不是滑块"))),
        }
    }
}
