// Hand-written override for `_InlinedRequest_to_mldev` (batches.py):
// the Python original passes `getv(to_object, ['request'], default_value={})`
// as a nested converter's `parent_object`, which the generator's DSL subset
// (tools/codegen/gen_converters.py) does not support (a computed sub-path,
// not a bare `to_object`/`parent_object`/`None`).
pub(crate) fn inlined_request_to_mldev(
    from_object: &Value,
    mut parent_object: Option<&mut Value>,
    _root_object: Option<&Value>,
) -> Result<Value> {
    let mut to_object = Value::Object(Map::new());

    if getv(Some(from_object), &["model"]).is_some() {
        let model = t::t_model(getv(Some(from_object), &["model"]).unwrap_or(Value::Null))?;
        setv(Some(&mut to_object), &["request", "model"], model)?;
    }

    if getv(Some(from_object), &["contents"]).is_some() {
        let contents = t::t_contents(getv(Some(from_object), &["contents"]).unwrap_or(Value::Null))?;
        let mut converted = Vec::new();
        for item in contents.as_array().cloned().unwrap_or_default() {
            converted.push(crate::converters::generated::models::content_to_mldev(&item, Some(&mut to_object), None)?);
        }
        setv(Some(&mut to_object), &["request", "contents"], Value::Array(converted))?;
    }

    if getv(Some(from_object), &["metadata"]).is_some() {
        let metadata = getv(Some(from_object), &["metadata"]).unwrap_or(Value::Null);
        setv(Some(&mut to_object), &["metadata"], metadata)?;
    }

    if getv(Some(from_object), &["config"]).is_some() {
        let config = getv(Some(from_object), &["config"]).unwrap_or(Value::Null);
        if let Some(obj) = to_object.as_object_mut() {
            obj.entry("request").or_insert_with(|| Value::Object(Map::new()));
        }
        let generation_config = {
            let request_obj = to_object.as_object_mut().and_then(|o| o.get_mut("request"));
            crate::converters::generated::models::generate_content_config_to_mldev(&config, request_obj, None)?
        };
        setv(Some(&mut to_object), &["request", "generationConfig"], generation_config)?;
    }

    Ok(to_object)
}
