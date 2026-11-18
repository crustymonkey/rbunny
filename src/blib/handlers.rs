use anyhow::{anyhow, Result};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Redirect},
    routing::get,
    Router,
};
use configparser::ini::Ini;
use minijinja::{context, Environment};
use std::{collections::HashMap, sync::Arc};
use urlencoding::encode;

// Shortcut typemap
type Smap = HashMap<String, String>;
type IniMap = HashMap<String, HashMap<String, Option<String>>>;

struct HState {
    ini: Arc<Ini>,
    tpls: Arc<Environment<'static>>,
}

pub fn get_routes(ini: Arc<Ini>, env: Arc<Environment<'static>>) -> Router {
    let state = Arc::new(HState {
        ini: ini.clone(),
        tpls: env.clone(),
    });
    let router = Router::new()
        .route("/", get(index))
        .route("/search", get(search))
        .route("/list", get(list))
        .route("/api/list", get(api_list))
        .route("/opensearch", get(opensearch))
        .route("/invalid", get(invalid))
        .with_state(state.clone());

    return router;
}

/*
 * The handlers for the different routes
 */
async fn index(State(state): State<Arc<HState>>) -> impl IntoResponse {
    let tpl = state
        .tpls
        .get_template("index.html")
        .expect("Failed to load template list.html");

    return Html(tpl.render(context!()).unwrap());
}

async fn search(Query(params): Query<Smap>, State(state): State<Arc<HState>>) -> impl IntoResponse {
    let query = match params.get("q") {
        Some(q) => q,
        None => {
            warn!("Invalid request: no \"q\" query param");
            return Redirect::temporary("/invalid");
        }
    };
    let parts: Vec<&str> = query.trim().split(' ').collect();
    if parts.len() < 1 {
        warn!("Invalid request: there's no shortcut specified");
        return Redirect::temporary("/invalid");
    }
    let map = state.ini.get_map_ref();
    let cmd = parts[0];

    if cmd != "main" && !map.contains_key(cmd) {
        // Default to a google search
        return Redirect::temporary(&process_tpl_url("g", map, &parts[..]).unwrap());
    } else if cmd == "main" {
        // Default to a google search as this is an invalid (reserved) command
        return Redirect::temporary(&process_tpl_url("g", map, &parts[..]).unwrap());
    }

    if parts.len() == 1 {
        // We have just a bare "command", so we need to check for a regular
        // "url" or "url_fallback" in the config. We start with a bare url
        if map[cmd].contains_key("url") {
            return Redirect::temporary(map[cmd]["url"].as_ref().unwrap());
        } else if map[cmd].contains_key("url_fallback") {
            return Redirect::temporary(map[cmd]["url_fallback"].as_ref().unwrap());
        } else {
            // This is just a 1 word search, default to a google search
            return Redirect::temporary(&process_tpl_url("g", map, &parts[..]).unwrap());
        }
    }

    // If we get here, we need to process a templated url
    let final_url = match process_tpl_url(cmd, map, &parts[1..]) {
        Ok(u) => u,
        Err(e) => {
            warn!("{}", &e.to_string());
            return Redirect::temporary("/invalid");
        }
    };

    return Redirect::temporary(&final_url);
}

async fn list(State(state): State<Arc<HState>>) -> impl IntoResponse {
    let tpl = state
        .tpls
        .get_template("list.html")
        .expect("Failed to load template list.html");

    return Html(
        tpl.render(context!(map => get_filtered_map(state.ini.get_map_ref())))
            .unwrap(),
    );
}

async fn api_list(State(state): State<Arc<HState>>) -> impl IntoResponse {
    let map = get_filtered_map(state.ini.get_map_ref());
    return Json(map);
}

async fn opensearch(State(state): State<Arc<HState>>) -> impl IntoResponse {
    let tpl = state
        .tpls
        .get_template("opensearch.xml")
        .expect("Failed to load the opensearch.xml template");
    return (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        tpl.render(context!()).unwrap(),
    );
}

async fn invalid() -> impl IntoResponse {
    return (StatusCode::BAD_REQUEST, "Invalid request");
}

/*
 * Utility functions
 */
fn process_tpl_url(cmd: &str, map: &IniMap, parts: &[&str]) -> Result<String> {
    let query = parts.join(" ");

    if !map[cmd].contains_key("url_tpl") {
        // We have a bad config if we get here because we *should* have
        // a "url_tpl" config
        return Err(anyhow!(format!(
            "Invalid configuration, no url_tpl for {}",
            cmd
        )));
    }

    let url_tpl = match &map[cmd]["url_tpl"] {
        Some(u) => u.to_string(),
        None => {
            return Err(anyhow!(format!(
                "Invalid configuration, blank config for {}",
                cmd
            )));
        }
    };

    if !url_tpl.contains("%s") {
        return Err(anyhow!(format!(
            "Invalid template config for {}, no '%s'",
            cmd
        )));
    }

    // Validation complete, let's process this sucker
    return Ok(url_tpl.replace("%s", &encode(&query)));
}

fn get_filtered_map(map: &IniMap) -> IniMap {
    let mut ret: IniMap = HashMap::new();
    for (key, val) in map.iter() {
        if key == "main" {
            // Skip the main section
            continue;
        }

        ret.insert(key.clone(), val.clone());
    }

    return ret;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_test_map() -> IniMap {
        let mut map = HashMap::new();
        let main = HashMap::from([
            ("bind_addr".to_string(), Some("127.0.0.1".to_string())),
            ("tpl_dir".to_string(), Some("/etc/tpl".to_string())),
        ]);
        let google = HashMap::from([
            (
                "url_fallback".to_string(),
                Some("https://www.google.com".to_string()),
            ),
            (
                "url_tpl".to_string(),
                Some("https://www.google.com/search?q=%s".to_string()),
            ),
        ]);
        let bad_value1 = HashMap::from([("url_tpl".to_string(), None)]);
        let bad_value2 = HashMap::from([("url".to_string(), Some("Irrelevant".to_string()))]);
        let bad_value3 = HashMap::from([(
            "url_tpl".to_string(),
            Some("https://www.google.com/search?q=".to_string()),
        )]);
        map.insert("main".to_string(), main);
        map.insert("g".to_string(), google);
        map.insert("b1".to_string(), bad_value1);
        map.insert("b2".to_string(), bad_value2);
        map.insert("b3".to_string(), bad_value3);

        return map;
    }

    #[test]
    fn test_process_tpl_url() {
        let map = get_test_map();
        let parts = vec!["lookup", "something"];

        assert_eq!(
            process_tpl_url("g", &map, &parts[..]).unwrap(),
            "https://www.google.com/search?q=lookup%20something",
        );

        assert!(process_tpl_url("b1", &map, &parts).is_err());
        assert!(process_tpl_url("b2", &map, &parts).is_err());
        assert!(process_tpl_url("b3", &map, &parts).is_err());
    }

    #[test]
    fn test_get_filtered_map() {
        let map = get_test_map();

        let filt = get_filtered_map(&map);
        assert!(!filt.contains_key("main"));
        assert!(filt.contains_key("g"));
    }
}
