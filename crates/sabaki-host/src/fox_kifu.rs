//! Fox Go Server (野狐围棋) SGF fetching and game list synchronization.
//!
//! Implements user nickname resolution, recent games retrieval, and SGF
//! downloading matching LizzieYZY's Fox Go client integration.

use serde::{Deserialize, Serialize};

pub const FOX_QUERY_USER_URL: &str = "https://newframe.foxwq.com/cgi/QueryUserInfoPanel";
pub const FOX_CHESS_LIST_URL: &str =
    "https://h5.foxwq.com/yehuDiamond/chessbook_local/YHWQFetchChessList";
pub const FOX_FETCH_CHESS_URL: &str =
    "https://h5.foxwq.com/yehuDiamond/chessbook_local/YHWQFetchChess";
pub const FOX_CGI_FETCH_CHESS_URL: &str =
    "http://cgi.foxwq.com/cgi-bin/CommonMobileCGI/TXWQFetchChess";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FoxUserSummary {
    pub uid: String,
    pub username: String,
    pub rank: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FoxGameSummary {
    pub chess_id: String,
    pub black_name: String,
    pub black_rank: String,
    pub white_name: String,
    pub white_rank: String,
    pub result: String,
    pub date: String,
    pub moves_count: usize,
}

/// Builds the URL for querying user account info by nickname.
pub fn build_query_user_url(nickname: &str) -> String {
    let encoded = url_encode(nickname);
    format!("{FOX_QUERY_USER_URL}?srcuid=0&username={encoded}")
}

/// Builds the URL for fetching the recent games list of a Fox account by UID.
pub fn build_fetch_chess_list_url(uid: &str, last_code: &str) -> String {
    let enc_uid = url_encode(uid);
    let enc_code = url_encode(last_code);
    format!(
        "{FOX_CHESS_LIST_URL}?srcuid=0&dstuid={enc_uid}&type=1&lastcode={enc_code}&searchkey=&uin={enc_uid}"
    )
}

/// Builds the URL for downloading an SGF game record by chess ID.
pub fn build_fetch_chess_url(chess_id: &str) -> String {
    let enc_id = url_encode(chess_id);
    format!("{FOX_FETCH_CHESS_URL}?chessid={enc_id}")
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.as_bytes() {
        match *byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// Parses the query user response JSON into a `FoxUserSummary`.
pub fn parse_query_user_response(json_str: &str) -> Result<FoxUserSummary, String> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid user query JSON: {e}"))?;

    let result = value
        .get("result")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    if result != 0 {
        let msg = value
            .get("resultstr")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("User not found");
        return Err(msg.to_owned());
    }

    let uid = value
        .get("uid")
        .map(|v| match v {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            _ => String::new(),
        })
        .unwrap_or_default();

    if uid.is_empty() {
        return Err("UID is empty".to_owned());
    }

    let username = value
        .get("name")
        .or_else(|| value.get("username"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let rank = value
        .get("rank")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();

    Ok(FoxUserSummary {
        uid,
        username,
        rank,
    })
}

/// Parses the Fox chess list response JSON and extracts game summaries.
pub fn parse_fox_chess_list_response(json_str: &str) -> Result<Vec<FoxGameSummary>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid chess list JSON: {e}"))?;

    let result = value
        .get("result")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    if result != 0 {
        return Err("Fox chess list request failed".to_owned());
    }

    let Some(list_val) = value.get("chesslist").or_else(|| value.get("list")) else {
        return Ok(Vec::new());
    };

    let Some(list) = list_val.as_array() else {
        return Ok(Vec::new());
    };

    let mut games = Vec::with_capacity(list.len());
    for item in list {
        let chess_id = item
            .get("chessid")
            .or_else(|| item.get("chess_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();

        if chess_id.is_empty() {
            continue;
        }

        let black_name = item
            .get("bkname")
            .or_else(|| item.get("black_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Black")
            .to_owned();

        let black_rank = item
            .get("bk_dan")
            .or_else(|| item.get("black_rank"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let white_name = item
            .get("wtname")
            .or_else(|| item.get("white_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("White")
            .to_owned();

        let white_rank = item
            .get("wt_dan")
            .or_else(|| item.get("white_rank"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let result = item
            .get("result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let date = item
            .get("time")
            .or_else(|| item.get("date"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let moves_count = item
            .get("step")
            .or_else(|| item.get("moves"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;

        games.push(FoxGameSummary {
            chess_id,
            black_name,
            black_rank,
            white_name,
            white_rank,
            result,
            date,
            moves_count,
        });
    }

    Ok(games)
}

/// Resource adapter used by the Fox task module. It is deliberately narrower
/// than a general network client: callers can only perform the GET requests
/// constructed by `FoxKifuClient`.
pub trait FoxHttpAdapter {
    fn get(&self, url: &str) -> Result<String, String>;
}

/// Production Fox transport. The command invocation is private to this adapter
/// so neither the shell nor Fox task callers need to know about `curl`.
pub struct CurlFoxHttpAdapter;

impl FoxHttpAdapter for CurlFoxHttpAdapter {
    fn get(&self, url: &str) -> Result<String, String> {
        let output = std::process::Command::new("curl")
            .arg("-s")
            .arg("-L")
            .arg("--max-time")
            .arg("10")
            .arg(url)
            .output()
            .map_err(|error| format!("curl command failed: {error}"))?;

        if !output.status.success() {
            return Err(format!(
                "HTTP request failed with exit code: {:?}",
                output.status.code()
            ));
        }

        String::from_utf8(output.stdout).map_err(|error| format!("non-utf8 HTTP response: {error}"))
    }
}

/// A deep task module for Fox lookup and SGF retrieval. Its Interface exposes
/// user-oriented operations while the URL construction, response parsing, and
/// transport remain implementation details behind one testable seam.
pub struct FoxKifuClient<A> {
    http: A,
}

impl<A> FoxKifuClient<A>
where
    A: FoxHttpAdapter,
{
    pub fn new(http: A) -> Self {
        Self { http }
    }

    /// Fetches recent games for a Fox nickname or numerical UID.
    pub fn fetch_recent_games(&self, nickname_or_uid: &str) -> Result<Vec<FoxGameSummary>, String> {
        let uid = if nickname_or_uid
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            nickname_or_uid.to_owned()
        } else {
            let user_json = self.http.get(&build_query_user_url(nickname_or_uid))?;
            parse_query_user_response(&user_json)?.uid
        };
        let list_json = self.http.get(&build_fetch_chess_list_url(&uid, "0"))?;
        parse_fox_chess_list_response(&list_json)
    }

    /// Fetches and sanitizes one SGF game record by Fox chess ID.
    pub fn fetch_sgf(&self, chess_id: &str) -> Result<String, String> {
        let json = self.http.get(&build_fetch_chess_url(chess_id))?;
        parse_fox_sgf_response(&json)
    }
}

/// Fetches the recent games of a Fox user using the production HTTP adapter.
pub fn fetch_user_recent_games(nickname_or_uid: &str) -> Result<Vec<FoxGameSummary>, String> {
    FoxKifuClient::new(CurlFoxHttpAdapter).fetch_recent_games(nickname_or_uid)
}

/// Fetches an SGF game record using the production HTTP adapter.
pub fn fetch_game_sgf(chess_id: &str) -> Result<String, String> {
    FoxKifuClient::new(CurlFoxHttpAdapter).fetch_sgf(chess_id)
}

/// Parses the Fox SGF response JSON and extracts clean SGF text.
pub fn parse_fox_sgf_response(json_str: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid SGF response JSON: {e}"))?;

    let result = value
        .get("result")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    if result != 0 {
        return Err("Fox SGF request failed".to_owned());
    }

    let sgf = value
        .get("chess")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "missing chess field in response".to_owned())?;

    Ok(sanitize_fox_sgf(sgf))
}

/// Sanitizes raw SGF text from Fox Go server by stripping BOM and stray backslashes.
pub fn sanitize_fox_sgf(sgf: &str) -> String {
    let text = sgf.trim().trim_start_matches('\u{feff}');
    let mut out = String::with_capacity(text.len());
    let mut inside_value = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if inside_value {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if ch == ']' {
                inside_value = false;
            }
            continue;
        }
        if ch == '\\' {
            continue;
        }
        out.push(ch);
        if ch == '[' {
            inside_value = true;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct FixtureHttpAdapter {
        responses: BTreeMap<String, String>,
    }

    impl FoxHttpAdapter for FixtureHttpAdapter {
        fn get(&self, url: &str) -> Result<String, String> {
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| format!("unexpected fixture URL: {url}"))
        }
    }

    #[test]
    fn builds_valid_fox_urls() {
        let user_url = build_query_user_url("柯洁九段");
        assert!(user_url.contains("username=%E6%9F%AF%E6%B4%81%E4%B9%9D%E6%AE%B5"));

        let list_url = build_fetch_chess_list_url("123456", "0");
        assert!(list_url.contains("dstuid=123456"));
        assert!(list_url.contains("lastcode=0"));

        let chess_url = build_fetch_chess_url("game_999");
        assert!(chess_url.contains("chessid=game_999"));
    }

    #[test]
    fn client_fetches_named_users_without_a_real_network_or_process() {
        let nickname = "潜伏";
        let uid = "987654";
        let client = FoxKifuClient::new(FixtureHttpAdapter {
            responses: BTreeMap::from([
                (
                    build_query_user_url(nickname),
                    format!(r#"{{"result":0,"uid":"{uid}","name":"{nickname}"}}"#),
                ),
                (
                    build_fetch_chess_list_url(uid, "0"),
                    r#"{"result":0,"chesslist":[{"chessid":"game-1","bkname":"Black","wtname":"White","step":42}]}"#.to_owned(),
                ),
            ]),
        });

        let games = client
            .fetch_recent_games(nickname)
            .expect("fixture fetch succeeds");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].chess_id, "game-1");
        assert_eq!(games[0].moves_count, 42);
    }

    #[test]
    fn client_sanitizes_sgf_without_a_real_network_or_process() {
        let chess_id = "game-1";
        let client = FoxKifuClient::new(FixtureHttpAdapter {
            responses: BTreeMap::from([(
                build_fetch_chess_url(chess_id),
                format!(r#"{{"result":0,"chess":"{}"}}"#, "\u{feff}(;SZ[19];B[dp])"),
            )]),
        });

        assert_eq!(
            client.fetch_sgf(chess_id).expect("fixture fetch succeeds"),
            "(;SZ[19];B[dp])"
        );
    }

    #[test]
    fn parses_user_query_response() {
        let json = r#"{"result":0,"uid":987654,"name":"潜伏","rank":"9段"}"#;
        let summary = parse_query_user_response(json).expect("valid JSON parses");
        assert_eq!(summary.uid, "987654");
        assert_eq!(summary.username, "潜伏");
        assert_eq!(summary.rank, "9段");
    }

    #[test]
    fn parses_and_sanitizes_fox_sgf() {
        let json = r#"{"result":0,"chess":"(;SZ[19]PB[柯洁]PW[申真谞];B[dp];W[pd])"}"#;
        let sgf = parse_fox_sgf_response(json).expect("valid SGF parses");
        assert!(sgf.contains("PB[柯洁]"));
        assert!(sgf.contains("PW[申真谞]"));
        assert!(sgf.contains(";B[dp]"));
    }

    #[test]
    fn parses_fox_chess_list_response() {
        let json = r#"{
            "result": 0,
            "chesslist": [
                {
                    "chessid": "20241012142512_8912",
                    "bkname": "柯洁",
                    "bk_dan": "9段",
                    "wtname": "申真谞",
                    "wt_dan": "9段",
                    "result": "黑中盘胜",
                    "time": "2024-10-12 14:25:12",
                    "step": 185
                },
                {
                    "chessid": "20241010091244_1234",
                    "bkname": "潜伏",
                    "bk_dan": "9段",
                    "wtname": "农心英雄",
                    "wt_dan": "9段",
                    "result": "白2.5目胜",
                    "time": "2024-10-10 09:12:44",
                    "step": 268
                }
            ]
        }"#;
        let games = parse_fox_chess_list_response(json).expect("valid chess list parses");
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].chess_id, "20241012142512_8912");
        assert_eq!(games[0].black_name, "柯洁");
        assert_eq!(games[0].black_rank, "9段");
        assert_eq!(games[0].white_name, "申真谞");
        assert_eq!(games[0].result, "黑中盘胜");
        assert_eq!(games[0].moves_count, 185);

        assert_eq!(games[1].black_name, "潜伏");
        assert_eq!(games[1].white_name, "农心英雄");
        assert_eq!(games[1].result, "白2.5目胜");
    }
}
