//! Fox Go Server (野狐围棋) SGF fetching and game list synchronization.
//!
//! Implements user nickname resolution, recent games retrieval, and SGF
//! downloading matching LizzieYZY's Fox Go client integration.

use serde::{Deserialize, Serialize};

pub const FOX_QUERY_USER_URL: &str = "https://newframe.foxwq.com/cgi/QueryUserInfoPanel";
pub const FOX_CHESS_LIST_URL: &str =
    "https://cgi.huanle.qq.com/cgi-bin/CommonMobileCGI/TXWQFetchChessList";
pub const FOX_FETCH_CHESS_URL: &str =
    "https://cgi.huanle.qq.com/cgi-bin/CommonMobileCGI/TXWQFetchChess";
pub const FOX_CGI_FETCH_CHESS_URL: &str =
    "https://cgi.huanle.qq.com/cgi-bin/CommonMobileCGI/TXWQFetchChess";

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
        "{FOX_CHESS_LIST_URL}?type=7&lastCode={enc_code}&lastcode={enc_code}&srcuid={enc_uid}&dstuid={enc_uid}&searchkey=&uin={enc_uid}&txwqsession=ryusei&fetchnum=20"
    )
}

/// Builds the URL for fetching recent games directly by nickname via the Tencent CGI.
pub fn build_query_user_chess_list_url(username: &str, last_code: &str) -> String {
    let enc_user = url_encode(username);
    let enc_code = url_encode(last_code);
    format!(
        "{FOX_CHESS_LIST_URL}?type=7&lastCode={enc_code}&lastcode={enc_code}&username={enc_user}&srcuid=0&dstuid=0&searchkey=&txwqsession=ryusei&fetchnum=20"
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

fn parse_rank_value(val: Option<&serde_json::Value>) -> String {
    let Some(val) = val else {
        return String::new();
    };
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => {
            if let Some(num) = n.as_i64() {
                if num >= 100 {
                    format!("P{}段", num - 100 + 1)
                } else if num >= 18 {
                    format!("{}D", num - 18 + 1)
                } else {
                    format!("{}K", 18 - num)
                }
            } else {
                n.to_string()
            }
        }
        _ => String::new(),
    }
}

fn parse_result_value(item: &serde_json::Value) -> String {
    if let Some(res) = item.get("result").and_then(serde_json::Value::as_str)
        && !res.is_empty()
    {
        return res.to_owned();
    }
    let winner = item
        .get("winner")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let reason = item
        .get("reason")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    match winner {
        1 => {
            if reason == 3 {
                "黑中盘胜".to_owned()
            } else {
                "黑胜".to_owned()
            }
        }
        2 => {
            if reason == 3 {
                "白中盘胜".to_owned()
            } else {
                "白胜".to_owned()
            }
        }
        _ => String::new(),
    }
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
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .unwrap_or_default();

        if chess_id.is_empty() {
            continue;
        }

        let black_name = item
            .get("blacknick")
            .or_else(|| item.get("bkname"))
            .or_else(|| item.get("black_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Black")
            .to_owned();

        let black_rank = parse_rank_value(
            item.get("blackdan")
                .or_else(|| item.get("bk_dan"))
                .or_else(|| item.get("black_rank")),
        );

        let white_name = item
            .get("whitenick")
            .or_else(|| item.get("wtname"))
            .or_else(|| item.get("white_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("White")
            .to_owned();

        let white_rank = parse_rank_value(
            item.get("whitedan")
                .or_else(|| item.get("wt_dan"))
                .or_else(|| item.get("white_rank")),
        );

        let result = parse_result_value(item);

        let date = item
            .get("starttime")
            .or_else(|| item.get("time"))
            .or_else(|| item.get("date"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let moves_count = item
            .get("movenum")
            .or_else(|| item.get("step"))
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
            .arg("-H")
            .arg("User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
            .arg("-H")
            .arg("Accept: application/json,text/plain,*/*")
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
        let trimmed = nickname_or_uid.trim();
        if trimmed.is_empty() || trimmed == "0" {
            let list_url = build_fetch_chess_list_url("0", "0");
            let list_json = self.http.get(&list_url)?;
            return parse_fox_chess_list_response(&list_json);
        }

        if trimmed.chars().all(|character| character.is_ascii_digit()) {
            let list_url = build_fetch_chess_list_url(trimmed, "0");
            let list_json = self.http.get(&list_url)?;
            return parse_fox_chess_list_response(&list_json);
        }

        // Direct query by username (Tencent Weiqi production endpoint)
        let direct_url = build_query_user_chess_list_url(trimmed, "0");
        if let Ok(list_json) = self.http.get(&direct_url)
            && let Ok(games) = parse_fox_chess_list_response(&list_json)
            && !games.is_empty()
        {
            return Ok(games);
        }

        // Fallback to legacy two-step user query (supports legacy fixture test)
        let user_url = build_query_user_url(trimmed);
        if let Ok(user_json) = self.http.get(&user_url)
            && let Ok(summary) = parse_query_user_response(&user_json)
        {
            let list_url = build_fetch_chess_list_url(&summary.uid, "0");
            let list_json = self.http.get(&list_url)?;
            return parse_fox_chess_list_response(&list_json);
        }

        Err(format!("未查询到野狐用户或对局: {trimmed}"))
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

    Ok(normalize_fox_komi(&sanitize_fox_sgf(sgf)))
}

/// Normalizes the Tencent Weiqi SGF komi encoding. The Tencent API stores komi
/// as an integer in 子 (stones) × 100 (e.g. `375` = 3.75 子 = 7.5 目), while the
/// SGF `KM` property is expressed in 目 (points). Convert integer `KM` values to
/// 目 by dividing by 50 so the imported game shows the correct komi.
fn normalize_fox_komi(sgf: &str) -> String {
    let mut result = String::with_capacity(sgf.len());
    let mut rest = sgf;
    while let Some(pos) = rest.find("KM[") {
        result.push_str(&rest[..pos + 3]);
        rest = &rest[pos + 3..];
        if let Some(close) = rest.find(']') {
            let value = &rest[..close];
            if let Ok(int_val) = value.parse::<i64>() {
                let komi = int_val as f64 / 50.0;
                result.push_str(&format_komi(komi));
            } else {
                result.push_str(value);
            }
            result.push(']');
            rest = &rest[close + 1..];
        } else {
            result.push_str(rest);
            rest = "";
        }
    }
    result.push_str(rest);
    result
}

fn format_komi(komi: f64) -> String {
    if (komi - komi.trunc()).abs() < f64::EPSILON {
        format!("{}", komi as i64)
    } else {
        format!("{:.1}", komi)
    }
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
    fn normalizes_tencent_komi_from_stones_to_points() {
        // Tencent stores komi as 子 × 100: 375 = 3.75 子 = 7.5 目.
        let json = r#"{"result":0,"chess":"(;GM[1]SZ[19]KM[375]HA[0]RU[Chinese];B[pd])"}"#;
        let sgf = parse_fox_sgf_response(json).expect("valid SGF parses");
        assert!(sgf.contains("KM[7.5]"), "komi must be 7.5 目, got: {sgf}");
        assert!(!sgf.contains("KM[375]"), "raw 子 encoding must be removed");

        // Zero komi (handicap) stays zero.
        let zero = r#"{"result":0,"chess":"(;SZ[19]KM[0]HA[2];B[pd])"}"#;
        let zero_sgf = parse_fox_sgf_response(zero).expect("valid SGF parses");
        assert!(zero_sgf.contains("KM[0]"));

        // Decimal komi (already in 目) is left untouched.
        let decimal = r#"{"result":0,"chess":"(;SZ[19]KM[6.5];B[pd])"}"#;
        let decimal_sgf = parse_fox_sgf_response(decimal).expect("valid SGF parses");
        assert!(decimal_sgf.contains("KM[6.5]"));
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

    #[test]
    fn parses_tencent_production_chess_list_response() {
        let json = r#"{
            "result": 0,
            "chesslist": [
                {
                    "chessid": "1785317779010001006",
                    "blackuid": 6757425,
                    "blacknick": "柯洁",
                    "blackdan": 108,
                    "whiteuid": 7093195,
                    "whitenick": "党毅飞",
                    "whitedan": 108,
                    "winner": 1,
                    "reason": 3,
                    "movenum": 205,
                    "starttime": "2026-07-29 17:36:19"
                }
            ]
        }"#;
        let games = parse_fox_chess_list_response(json).expect("valid Tencent response parses");
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.chess_id, "1785317779010001006");
        assert_eq!(g.black_name, "柯洁");
        assert_eq!(g.black_rank, "P9段");
        assert_eq!(g.white_name, "党毅飞");
        assert_eq!(g.white_rank, "P9段");
        assert_eq!(g.result, "黑中盘胜");
        assert_eq!(g.moves_count, 205);
        assert_eq!(g.date, "2026-07-29 17:36:19");
    }

    #[test]
    fn client_fetches_via_tencent_direct_query() {
        let nickname = "柯洁";
        let direct_url = build_query_user_chess_list_url(nickname, "0");
        let client = FoxKifuClient::new(FixtureHttpAdapter {
            responses: BTreeMap::from([(
                direct_url,
                r#"{"result":0,"chesslist":[{"chessid":"tencent-1","blacknick":"柯洁","blackdan":108,"whitenick":"党毅飞","whitedan":108,"winner":1,"reason":3,"movenum":190}]}"#.to_owned(),
            )]),
        });

        let games = client
            .fetch_recent_games(nickname)
            .expect("tencent direct query succeeds");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].chess_id, "tencent-1");
        assert_eq!(games[0].black_name, "柯洁");
        assert_eq!(games[0].black_rank, "P9段");
        assert_eq!(games[0].moves_count, 190);
    }

    #[test]
    #[ignore]
    fn online_fetch_ke_jie_game_and_sgf() {
        let games = fetch_user_recent_games("柯洁").expect("online fetch games");
        assert!(!games.is_empty(), "must find games for 柯洁");
        let sgf = fetch_game_sgf(&games[0].chess_id).expect("online fetch sgf");
        assert!(
            sgf.contains("(;GM[1]") || sgf.contains("(;SZ[19]") || sgf.contains("(;"),
            "valid SGF content"
        );
    }

    #[test]
    #[ignore]
    fn online_fetch_global_recent_games_and_sgf() {
        let games = fetch_user_recent_games("").expect("online fetch global recent games");
        assert!(!games.is_empty(), "must find global games");
        let sgf = fetch_game_sgf(&games[0].chess_id).expect("online fetch sgf");
        assert!(sgf.contains("(;"), "valid SGF content");
    }
}
