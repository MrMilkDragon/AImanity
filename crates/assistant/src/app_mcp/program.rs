use rhai::{Dynamic, Engine, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize)]
pub(super) struct Program {
    pub name: String,
    pub description: String,
    pub input_type: String,
    pub input_description: String,
    #[serde(default)]
    pub uses_view: bool,
    pub source: String,
    pub examples: Vec<Example>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Example {
    input_json: String,
    expected_json: String,
    #[serde(default)]
    view_json: Option<String>,
}

impl Program {
    pub fn schema(&self) -> Value {
        let mut input = json!({"type":self.input_type,"description":self.input_description});
        if self.input_type == "array" {
            input["items"] = json!({});
        }
        json!({"type":"object","properties":{"input":input},
            "required":["input"],"additionalProperties":false})
    }

    fn engine(&self) -> Result<(Engine, rhai::AST), String> {
        let mut engine = Engine::new();
        engine
            .disable_symbol("eval")
            .disable_symbol("print")
            .disable_symbol("debug");
        engine
            .set_max_operations(30_000)
            .set_max_string_size(65_536)
            .set_max_array_size(4096)
            .set_max_map_size(1024)
            .set_max_call_levels(16);
        let ast = engine.compile(&self.source).map_err(|e| e.to_string())?;
        Ok((engine, ast))
    }

    fn evaluate(
        &self,
        engine: &Engine,
        ast: &rhai::AST,
        input: &Value,
        view: Option<&Value>,
    ) -> Result<Value, String> {
        let matches = match self.input_type.as_str() {
            "array" => input.is_array(),
            "object" => input.is_object(),
            "string" => input.is_string(),
            "number" => input.is_number(),
            "integer" => input.is_i64() || input.is_u64(),
            "boolean" => input.is_boolean(),
            _ => false,
        };
        if !matches || input.to_string().len() > 65_536 {
            return Err("input does not match the tool schema or exceeds 64 KiB".into());
        }
        let input = rhai::serde::to_dynamic(input).map_err(|e| e.to_string())?;
        let mut scope = Scope::new();
        scope.push_constant("input", input);
        if self.uses_view {
            let view = view.ok_or("observe the application before calling this tool")?;
            scope.push_constant(
                "view",
                rhai::serde::to_dynamic(view).map_err(|e| e.to_string())?,
            );
        }
        let value = engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, ast)
            .map_err(|e| e.to_string())?;
        let output = rhai::serde::from_dynamic::<Value>(&value).map_err(|e| e.to_string())?;
        if output.to_string().len() > 65_536 {
            return Err("tool output exceeds 64 KiB".into());
        }
        Ok(output)
    }

    pub fn verify(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.len() > 48
            || !self
                .name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            || self.description.trim().is_empty()
            || self.description.len() > 480
            || self.description.contains(['\r', '\n'])
            || self.input_description.len() > 480
            || self.source.len() > 8192
            || self.examples.len() != 2
        {
            return Err("invalid app tool definition".into());
        }
        let (engine, ast) = self.engine()?;
        let mut previous = None;
        for example in &self.examples {
            let input: Value =
                serde_json::from_str(&example.input_json).map_err(|e| e.to_string())?;
            let expected: Value =
                serde_json::from_str(&example.expected_json).map_err(|e| e.to_string())?;
            let view: Option<Value> = example
                .view_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| e.to_string())?;
            if previous
                .as_ref()
                .is_some_and(|(a, b, c)| (a == &input && c == &view) || b == &expected)
                || self.evaluate(&engine, &ast, &input, view.as_ref())? != expected
            {
                return Err("app tool did not pass two distinct examples".into());
            }
            previous = Some((input, expected, view));
        }
        Ok(())
    }

    pub fn call(&self, args: &Value, view: Option<&Value>) -> Result<Value, String> {
        if args
            .as_object()
            .is_none_or(|a| a.len() != 1 || !a.contains_key("input"))
        {
            return Err("expected one input argument".into());
        }
        let (engine, ast) = self.engine()?;
        self.evaluate(&engine, &ast, &args["input"], view)
    }

    pub fn document(&self, name: &str) -> Result<String, String> {
        let program = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        Ok(format!(
            "---\nname: {name}\ndescription: {}\n---\n\n```json\n{program}\n```\n",
            self.description
        ))
    }

    pub fn read(document: &str) -> Result<Self, String> {
        let (_, code) = document
            .split_once("\n```json\n")
            .ok_or("missing MCP program")?;
        serde_json::from_str(
            code.trim_end()
                .strip_suffix("\n```")
                .ok_or("incomplete MCP program")?,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn view_program_uses_the_supplied_controls_and_rejects_a_constant_result() {
        let mut p:Program=serde_json::from_value(json!({
            "name":"find_search","description":"Find the search control","input_type":"string",
            "input_description":"resource ID","uses_view":true,
            "source":"for node in view.elements { if node.resource_id == input { return node.id; } } ()",
            "examples":[{"input_json":"\"search\"","view_json":"{\"elements\":[{\"resource_id\":\"search\",\"id\":\"one\"}]}","expected_json":"\"one\""},
                {"input_json":"\"search\"","view_json":"{\"elements\":[]}","expected_json":"null"}]
        })).unwrap();
        p.verify().unwrap();
        let args = json!({"input":"search"});
        assert!(p.call(&args, None).is_err());
        assert_eq!(
            p.call(
                &args,
                Some(&json!({"elements":[{"resource_id":"search","id":"another-frame"}]}))
            )
            .unwrap(),
            "another-frame"
        );
        p.source = "\"one\"".into();
        assert!(p.verify().is_err());
        p.source = "loop {}".into();
        assert!(p.call(&args, Some(&json!({}))).is_err());
    }
}
