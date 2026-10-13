use super::*;

/// Every pick the server sends comes back as the same pick.
#[test]
fn picks_round_trip_through_json() {
    for p in [
        Pick::Nothing,
        Pick::Task {
            project: "a".into(),
            target: "a:5".into(),
            retry: true,
        },
        Pick::Manual {
            project: "a".into(),
        },
        Pick::Run {
            id: 3,
            clean: true,
            state: "ready".into(),
            project: "socket.io".into(),
        },
        Pick::Job {
            id: 4,
            awaiting: false,
        },
        Pick::Instance {
            id: 5,
            workflow: "w".into(),
            open: true,
        },
        Pick::Note {
            id: 6,
            text: "t".into(),
        },
        Pick::Campaign { name: "c".into() },
    ] {
        assert_eq!(pick_from(&pick_json(&p)), Some(p));
    }
    assert_eq!(pick_from(&json!({ "kind": "run", "id": "3" })), None);
    assert_eq!(pick_from(&json!({ "kind": "shell" })), None);
}

/// The page learns every tab and every act from the table, by the names the
/// server parses back.
#[test]
fn the_key_table_reaches_the_page_whole() {
    let k = keys_json();
    let tabs: Vec<&str> = k["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for t in Tab::ALL {
        assert!(tabs.contains(&t.title()));
        assert_eq!(tab_named(t.title()), Some(t));
    }
    for a in Act::ALL {
        assert!(
            k["bindings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["command"]["act"] == a.name()),
            "{a:?}"
        );
        assert_eq!(act_named(&json!(a.name())), Some(a));
    }
}

#[test]
fn only_loopback_names_are_hosts() {
    for ok in ["127.0.0.1:8080", "localhost:9000"] {
        assert!(loopback(ok), "{ok}");
    }
    for bad in [
        "evil.example:8080",
        "127.0.0.1",
        "127.0.0.1:port",
        "127.0.0.1.evil.example:80",
        "localhost.evil.example:80",
        "0.0.0.0:8080",
    ] {
        assert!(!loopback(bad), "{bad}");
    }
}

#[test]
fn tokens_are_compared_whole() {
    assert!(same("abc", "abc"));
    assert!(!same("abc", "abd"));
    assert!(!same("abc", "ab"));
    let t = token().unwrap();
    assert_eq!(t.len(), 64);
    assert_ne!(t, token().unwrap());
}

/// The page handles every kind of command the table holds, so no key works
/// in the TUI and does nothing in the browser.
#[test]
fn the_page_handles_every_command() {
    let mut kinds: Vec<String> = keys_json()["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["command"]["kind"].as_str().unwrap().to_string())
        .collect();
    kinds.sort();
    kinds.dedup();
    for kind in &kinds {
        assert!(
            SCRIPT.contains(&format!("case \"{kind}\"")),
            "app.js has no case for `{kind}`"
        );
    }
    assert!(kinds.len() >= 12, "{kinds:?}");
}
