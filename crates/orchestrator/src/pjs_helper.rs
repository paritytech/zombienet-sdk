use anyhow::anyhow;
pub use pjs_rs::ReturnValue;
use serde_json::json;
use tracing::trace;

pub fn pjs_build_template(
    ws_uri: &str,
    content: &str,
    args: Vec<serde_json::Value>,
    user_types: Option<serde_json::Value>,
) -> String {
    let types = if let Some(user_types) = user_types {
        if let Some(types) = user_types.pointer("/types") {
            // if the user_types includes the `types` key use the inner value
            types.clone()
        } else {
            user_types.clone()
        }
    } else {
        // No custom types, just an emtpy json
        json!({})
    };

    let tmpl = format!(
        r#"
    const {{ util, utilCrypto, keyring, types }} = pjs;
    ( async () => {{
        const api = await pjs.api.ApiPromise.create({{
            provider: new pjs.api.WsProvider('{}'),
            types: {}
         }});
        const _run = async (api, hashing, keyring, types, util, arguments) => {{
            {}
        }};
        return await _run(api, utilCrypto, keyring, types, util, {});
    }})()
    "#,
        ws_uri,
        types,
        content,
        json!(args),
    );
    trace!(tmpl = tmpl, "code to execute");
    tmpl
}

// Since pjs-rs run a custom javascript runtime (using deno_core) we need to
// execute in an isolated thread.
pub fn pjs_exec(code: String) -> Result<ReturnValue, anyhow::Error> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    std::thread::spawn(move || {
        rt.block_on(async move {
            let value = pjs_rs::run_ts_code(code, None).await;
            trace!("ts_code return: {:?}", value);
            value
        })
    })
    .join()
    .map_err(|_| anyhow!("[pjs] Thread panicked"))?
}

/// pjs-rs success [Result] type
///
/// Represent the possible states returned from a succefully call to pjs-rs
///
/// Ok(value) -> Deserialized return value into a [serde_json::Value]
/// Err(msg) -> Execution of the script finish Ok, but the returned value
/// can't be deserialize into a [serde_json::Value]
pub type PjsResult = Result<serde_json::Value, String>;
