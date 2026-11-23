//! Локальный HTTP-сервер на 127.0.0.1 для источника «Браузер» в OBS: страница плеера (`/`),
//! её картинки, обложка текущего трека (`/cover`) и поток событий SSE (`/events`).
//! Свой минимальный HTTP: на каждое соединение — поток, который спит в блокирующем чтении
//! или на канале, пока не сменится трек.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use crate::hub::SharedHub;

struct Asset {
    path: &'static str,
    mime: &'static str,
    body: &'static [u8],
}

macro_rules! asset {
    ($path:literal, $mime:literal, $file:literal) => {
        Asset {
            path: $path,
            mime: $mime,
            body: include_bytes!(concat!("../assets/web/", $file)),
        }
    };
}

static ASSETS: &[Asset] = &[
    asset!("/", "text/html; charset=utf-8", "player.html"),
    asset!("/bg.webp", "image/webp", "bg.webp"),
    asset!("/glitch.webp", "image/webp", "glitch.webp"),
    asset!("/vinyl.webp", "image/webp", "vinyl.webp"),
    asset!("/imperium-bg.webp", "image/webp", "imperium-bg.webp"),
    asset!("/imperium-crest.webp", "image/webp", "imperium-crest.webp"),
    asset!("/neon-bg.webp", "image/webp", "neon-bg.webp"),
];

/// Запускает сервер. Ошибка — порт занят.
pub fn spawn(port: u16, hub: SharedHub) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("Порт {port} занят ({e}). Поменяйте port в config.json."))?;
    thread::Builder::new()
        .name("http".into())
        .stack_size(128 * 1024)
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let hub = hub.clone();
                let _ = thread::Builder::new()
                    .name("conn".into())
                    .stack_size(128 * 1024)
                    .spawn(move || handle(stream, hub));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn handle(stream: TcpStream, hub: SharedHub) {
    let _ = stream.set_nodelay(true);
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let path = target.split('?').next().unwrap_or("/");
    // Заголовки запроса не нужны — дочитываем до пустой строки.
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
    }
    let mut out = stream;
    if method != "GET" {
        let _ = respond(&mut out, "405 Method Not Allowed", "text/plain", b"", false);
        return;
    }
    match path {
        "/events" => events(out, hub),
        "/cover" => match hub.cover() {
            // Адрес обложки содержит её версию (?v=N) — браузер может кешировать.
            Some((mime, bytes)) => {
                let _ = respond(&mut out, "200 OK", &mime, &bytes, true);
            }
            None => {
                let _ = respond(&mut out, "404 Not Found", "text/plain", b"", false);
            }
        },
        _ => match ASSETS.iter().find(|a| a.path == path) {
            Some(a) => {
                let cache = a.mime.starts_with("image/");
                let _ = respond(&mut out, "200 OK", a.mime, a.body, cache);
            }
            None => {
                let _ = respond(&mut out, "404 Not Found", "text/plain", b"not found", false);
            }
        },
    }
}

fn respond(
    out: &mut TcpStream,
    status: &str,
    mime: &str,
    body: &[u8],
    cache: bool,
) -> std::io::Result<()> {
    let cache = if cache { "max-age=86400" } else { "no-cache" };
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    out.write_all(body)?;
    out.flush()
}

/// Server-Sent Events: соединение висит открытым, каждое изменение трека — одна строка
/// `data: {json}`. Браузер сам переподключается, если программу перезапустили.
fn events(mut out: TcpStream, hub: SharedHub) {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\nretry: 2000\n\n";
    if out.write_all(head.as_bytes()).is_err() {
        return;
    }
    let rx = hub.subscribe();
    // recv() вернёт ошибку только вместе с Hub — то есть никогда; выходим по ошибке записи.
    while let Ok(msg) = rx.recv() {
        let frame = format!("data: {msg}\n\n");
        if out
            .write_all(frame.as_bytes())
            .and_then(|_| out.flush())
            .is_err()
        {
            break;
        }
    }
}
