// Hand-written override for `_EmbedContentBatch_to_mldev` (batches.py):
// the Python original uses `move_value_by_path` (`movev`), a third DSL
// primitive the generator (tools/codegen/gen_converters.py) does not
// support. `_EmbedContentConfig_to_mldev` broadcasts its fields onto
// `to_object.requests[].{taskType,title,outputDimensionality}` (via
// `parent_object`); this override relocates them under
// `requests[].request.*`, next to `content`, matching the Python
// `movev(to_object, {'requests[].*': 'requests[].request.*'})` call.
pub(crate) fn embed_content_batch_to_mldev(
    from_object: &Value,
    mut parent_object: Option<&mut Value>,
    _root_object: Option<&Value>,
) -> Result<Value> {
    let mut to_object = Value::Object(Map::new());

    if getv(Some(from_object), &["contents"]).is_some() {
        let contents = t::t_contents_for_embed(getv(Some(from_object), &["contents"]).unwrap_or(Value::Null))?;
        let items = contents.as_array().cloned().unwrap_or_default();
        setv(Some(&mut to_object), &["requests[]", "request", "content"], Value::Array(items))?;
    }

    if getv(Some(from_object), &["config"]).is_some() {
        let config = getv(Some(from_object), &["config"]).unwrap_or(Value::Null);
        crate::converters::generated::batches::embed_content_config_to_mldev(&config, Some(&mut to_object), None)?;

        if let Some(Value::Array(requests)) = to_object.as_object_mut().and_then(|o| o.get_mut("requests")) {
            for item in requests.iter_mut() {
                let Value::Object(item_obj) = item else { continue };
                let broadcast_keys: Vec<String> = item_obj.keys().filter(|k| k.as_str() != "request").cloned().collect();
                if broadcast_keys.is_empty() {
                    continue;
                }
                let mut moved = Map::new();
                for key in broadcast_keys {
                    if let Some(value) = item_obj.remove(&key) {
                        moved.insert(key, value);
                    }
                }
                let request_entry = item_obj.entry("request".to_owned()).or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(request_map) = request_entry {
                    request_map.extend(moved);
                }
            }
        }
    }

    Ok(to_object)
}
