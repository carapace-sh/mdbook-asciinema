use regex::Regex;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
struct CastEvent {
    interval: f64,
    code: String,
    data: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "supports" {
        std::process::exit(0);
    }

    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let parsed: Value = serde_json::from_str(&input).unwrap();
    let arr = parsed.as_array().unwrap();
    let ctx = &arr[0];
    let mut book = arr[1].clone();

    let root = ctx.get("root").and_then(|v| v.as_str()).unwrap_or("");
    let src_dir = ctx
        .get("config")
        .and_then(|c| c.get("book"))
        .and_then(|b| b.get("src"))
        .and_then(|v| v.as_str())
        .unwrap_or("src");

    let pre_cfg = ctx
        .get("config")
        .and_then(|c| c.get("preprocessor"))
        .and_then(|p| p.get("asciinema"));
    let version = pre_cfg
        .and_then(|c| c.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let download = pre_cfg
        .and_then(|c| c.get("download"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if download {
        ensure_assets(root, version.as_deref());
    }

    process_book(&mut book, root, src_dir);

    println!("{}", serde_json::to_string(&book).unwrap());
}

fn ensure_assets(root: &str, version: Option<&str>) {
    if root.is_empty() {
        return;
    }
    let dir = Path::new(root).join("asciinema");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("mdbook-asciinema: {}", e);
        return;
    }
    let files = ["asciinema-player.min.js", "asciinema-player.css"];
    let base = match version {
        Some(v) if !v.is_empty() && v != "latest" => {
            let tag = if v.starts_with('v') {
                v.to_string()
            } else {
                format!("v{}", v)
            };
            format!(
                "https://github.com/asciinema/asciinema-player/releases/download/{}/",
                tag
            )
        }
        _ => "https://github.com/asciinema/asciinema-player/releases/latest/download/".to_string(),
    };
    for file in files {
        let path = dir.join(file);
        if path.exists() {
            continue;
        }
        let url = format!("{}{}", base, file);
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&path)
            .arg(&url)
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => eprintln!("mdbook-asciinema: failed to download {}", url),
        }
    }
}

fn process_book(book: &mut Value, root: &str, src_dir: &str) {
    if let Some(items) = book.get_mut("items").and_then(|v| v.as_array_mut()) {
        let n = items.len();
        for i in 0..n {
            process_item(&mut items[i], root, src_dir);
        }
    }
}

fn process_item(item: &mut Value, root: &str, src_dir: &str) {
    if let Some(ch) = item.get_mut("Chapter") {
        process_chapter(ch, root, src_dir);
        if let Some(subs) = ch.get_mut("sub_items").and_then(|v| v.as_array_mut()) {
            let n = subs.len();
            for i in 0..n {
                process_item(&mut subs[i], root, src_dir);
            }
        }
    }
}

fn process_chapter(ch: &mut Value, root: &str, src_dir: &str) {
    let content = ch
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if content.is_empty() {
        return;
    }
    let source_path = ch
        .get("source_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let base_dir = Path::new(source_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let base_dir = if base_dir == "." || base_dir.is_empty() {
        String::new()
    } else {
        base_dir
    };

    let (new_content, changed) = transform_content(&content, root, src_dir, &base_dir);
    if changed {
        ch["content"] = Value::String(new_content);
    }
}

fn transform_content(content: &str, root: &str, src_dir: &str, base_dir: &str) -> (String, bool) {
    let asset_prefix = if base_dir.is_empty() {
        String::new()
    } else {
        let depth = base_dir.split('/').filter(|s| !s.is_empty()).count();
        "../".repeat(depth)
    };

    let re = Regex::new(r#"!\[([^\]]*)\]\(([^)]+\.cast)(?:\s+"[^"]*")?\)"#).unwrap();
    let mut changed = false;
    let result = re.replace_all(content, |caps: &regex::Captures| {
        changed = true;
        let alt = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let src = caps.get(2).map(|m| m.as_str()).unwrap_or("").trim();

        let cast_rel = if base_dir.is_empty() {
            src.to_string()
        } else {
            Path::new(base_dir).join(src).to_string_lossy().to_string()
        };
        let cast_abs = Path::new(root)
            .join(src_dir)
            .join(&cast_rel)
            .to_string_lossy()
            .to_string();

        let (mut opts, preview_time, explicit_poster, has_time) = parse_config(alt, &cast_abs);

        if !explicit_poster && has_time {
            match generate_preview(&cast_abs, preview_time) {
                Ok(txt) => {
                    opts.insert("poster".to_string(), Value::String(format!("data:text/plain,{}", txt)));
                }
                Err(e) => {
                    eprintln!("mdbook-asciinema: {}", e);
                }
            }
        }

        if !opts.contains_key("cols") {
            let (c, _, _) = parse_cast(&cast_abs).unwrap_or((0, 0, vec![]));
            let c = if c == 0 { 108 } else { c };
            opts.insert("cols".to_string(), json!(c));
        }
        if !opts.contains_key("rows") {
            let (_, r, _) = parse_cast(&cast_abs).unwrap_or((0, 0, vec![]));
            let r = if r == 0 { 24 } else { r };
            opts.insert("rows".to_string(), json!(r));
        }

        let id = format!("asciinema-{}", COUNTER.fetch_add(1, Ordering::SeqCst));

        let src_json = serde_json::to_string(src).unwrap();
        let id_json = serde_json::to_string(&id).unwrap();
        let opts_json = serde_json::to_string(&Value::Object(opts.clone())).unwrap();

        format!(
            "<div id={} class=\"asciinema-player\"></div>\n<script>\nwindow.addEventListener(\"load\", function () {{\n  AsciinemaPlayer.create({}, document.getElementById({}), {});\n}});\n</script>",
            id_json, src_json, id_json, opts_json
        )
    });

    let result = result.to_string();
    if result != content {
        changed = true;
    }
    if changed {
        let tags = format!(
            "<link rel=\"stylesheet\" href=\"{}asciinema/asciinema-player.css\">\n<script src=\"{}asciinema/asciinema-player.min.js\"></script>\n",
            asset_prefix, asset_prefix
        );
        return (format!("{}{}", tags, result), true);
    }
    (result, changed)
}

fn parse_config(alt: &str, _cast_abs: &str) -> (serde_json::Map<String, Value>, f64, bool, bool) {
    let mut opts = serde_json::Map::new();
    let alt = alt.trim();
    let mut explicit_poster = false;
    let mut preview_time = 0.0;
    let mut has_preview_time = false;

    // @time extraction
    let mut alt_work = alt.to_string();
    if let Some(idx) = alt.find('@') {
        let rest = alt[idx + 1..].trim();
        let mut fields: Vec<&str> = rest.split_whitespace().collect();
        if !fields.is_empty() {
            preview_time = parse_time(fields[0]);
            has_preview_time = true;
            fields.remove(0);
            alt_work = format!("{} {}", &alt[..idx], fields.join(" ")).trim().to_string();
        }
    }

    // token parsing
    let tokens: Vec<&str> = alt_work
        .split(|c: char| c == ' ' || c == ',' || c == ';')
        .filter(|s| !s.trim().is_empty())
        .collect();
    let re_wh = Regex::new(r"^(\d+)\s*[xX]\s*(\d+)$").unwrap();
    for tok in tokens {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        if let Some(caps) = re_wh.captures(tok) {
            let cols: i64 = caps[1].parse().unwrap_or(0);
            let rows: i64 = caps[2].parse().unwrap_or(0);
            opts.insert("cols".to_string(), json!(cols));
            opts.insert("rows".to_string(), json!(rows));
            continue;
        }
        if tok.contains('=') {
            let mut parts = tok.splitn(2, '=');
            let k = parts.next().unwrap_or("").trim().to_string();
            let v = parts.next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').to_string();
            match k.to_lowercase().as_str() {
                "cols" | "columns" | "width" => {
                    if let Ok(n) = v.parse::<i64>() {
                        opts.insert("cols".to_string(), json!(n));
                    }
                }
                "rows" | "height" => {
                    if let Ok(n) = v.parse::<i64>() {
                        opts.insert("rows".to_string(), json!(n));
                    }
                }
                "time" | "preview" | "position" => {
                    preview_time = parse_time(&v);
                    has_preview_time = true;
                }
                "poster" => {
                    opts.insert("poster".to_string(), Value::String(v));
                    explicit_poster = true;
                }
                "startat" | "start_at" => {
                    if let Ok(f) = v.parse::<f64>() {
                        opts.insert("startAt".to_string(), json!(f));
                        if !has_preview_time {
                            preview_time = f;
                            has_preview_time = true;
                        }
                    } else {
                        opts.insert("startAt".to_string(), Value::String(v));
                    }
                }
                _ => {
                    opts.insert(k.clone(), parse_value(&k, &v));
                }
            }
            continue;
        }
    }

    if !opts.contains_key("cols") {
        let re2 = Regex::new(r"(\d+)\s*[xX]\s*(\d+)").unwrap();
        if let Some(caps) = re2.captures(&alt_work) {
            let cols: i64 = caps[1].parse().unwrap_or(0);
            let rows: i64 = caps[2].parse().unwrap_or(0);
            opts.insert("cols".to_string(), json!(cols));
            opts.insert("rows".to_string(), json!(rows));
        }
    }

    if !has_preview_time {
        if let Some(st) = opts.get("startAt").cloned() {
            if let Some(f) = to_float(&st) {
                preview_time = f;
                has_preview_time = true;
            }
        }
    }
    if !has_preview_time {
        preview_time = 0.0;
    }

    if opts.contains_key("poster") {
        explicit_poster = true;
    }

    (opts, preview_time, explicit_poster, has_preview_time)
}

fn parse_value(key: &str, val: &str) -> Value {
    if val.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if val.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    let lk = key.to_lowercase();
    let numeric_keys = [
        "speed", "idletimelimit", "idle_time_limit", "startat", "start_at",
        "cols", "rows", "terminalfontsize", "terminal_font_size",
    ];
    if numeric_keys.contains(&lk.as_str()) {
        if lk == "cols" || lk == "rows" {
            if let Ok(n) = val.parse::<i64>() {
                return json!(n);
            }
        }
        if let Ok(f) = val.parse::<f64>() {
            return json!(f);
        }
    }
    if let Ok(f) = val.parse::<f64>() {
        if val.contains('.') {
            return json!(f);
        }
        if let Ok(n) = val.parse::<i64>() {
            return json!(n);
        }
    }
    Value::String(val.to_string())
}

fn to_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn parse_cast(path: &str) -> Result<(i64, i64, Vec<CastEvent>), String> {
    let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut cols = 0;
    let mut rows = 0;
    let mut events = Vec::new();
    let mut first = true;
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if first {
            first = false;
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                if let Some(term) = v.get("term") {
                    cols = term.get("cols").and_then(|x| x.as_i64()).unwrap_or(0);
                    rows = term.get("rows").and_then(|x| x.as_i64()).unwrap_or(0);
                }
                if cols == 0 {
                    cols = v.get("cols").and_then(|x| x.as_i64()).unwrap_or(0);
                }
                if rows == 0 {
                    rows = v.get("rows").and_then(|x| x.as_i64()).unwrap_or(0);
                }
            }
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(arr) = v.as_array() {
                if arr.len() >= 3 {
                    let interval = arr[0].as_f64().unwrap_or(0.0);
                    let code = arr[1].as_str().unwrap_or("").to_string();
                    let data = arr[2].as_str().unwrap_or("").to_string();
                    events.push(CastEvent {
                        interval,
                        code,
                        data,
                    });
                }
            }
        }
    }
    Ok((cols, rows, events))
}

fn generate_preview(path: &str, target: f64) -> Result<String, String> {
    let (_, _, events) = parse_cast(path)?;
    let mut sb = String::new();
    let mut cum = 0.0;
    for e in events {
        cum += e.interval;
        if e.code != "o" {
            if cum > target {
                break;
            }
            continue;
        }
        if cum <= target + 1e-9 {
            sb.push_str(&e.data);
        } else {
            break;
        }
    }
    Ok(sb)
}

fn parse_time(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    let parts: Vec<&str> = s.split(':').collect();
    let mut total = 0.0;
    let mut mult = 1.0;
    for i in (0..parts.len()).rev() {
        let v: f64 = parts[i].trim().parse().unwrap_or(0.0);
        total += v * mult;
        mult *= 60.0;
    }
    total
}
