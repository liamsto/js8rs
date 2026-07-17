use js8rs::command::{CommandArg, CommandKind, Target, is_buffered_token, parse_command};

fn parsed(text: &str) -> js8rs::command::ParsedCommand<'_> {
    parse_command(text, None).unwrap_or_else(|| panic!("expected command in {text:?}"))
}

#[test]
fn parses_sender_recipient_group_and_payload() {
    let command = parsed("kn4crd: dr4cnk info 50w vert");
    assert_eq!(command.sender, Some("kn4crd"));
    assert_eq!(command.target, Some(Target::Call("dr4cnk")));
    assert_eq!(command.kind, CommandKind::Info);
    assert_eq!(command.command, "info");
    assert_eq!(command.payload, "50w vert");

    let group = parsed("@ARESGA SNR?");
    assert_eq!(group.target, Some(Target::Group("@ARESGA")));
    assert_eq!(group.kind, CommandKind::SnrQuery);
}

#[test]
fn parses_full_and_partial_relay_routes() {
    let command = parsed("N0JDS > OH8STN > KN4CRD MSG HELLO");
    let Some(Target::Relay(route)) = command.target else {
        panic!("expected full relay route");
    };
    assert!(!route.is_partial());
    assert_eq!(route.path(), "N0JDS > OH8STN > KN4CRD");
    assert_eq!(route.recipient(), "KN4CRD");
    assert_eq!(
        route.hops().collect::<Vec<_>>(),
        ["N0JDS", "OH8STN", "KN4CRD"]
    );
    assert_eq!(command.kind, CommandKind::Msg);
    assert_eq!(command.payload, "HELLO");

    let command = parse_command(">oh8stn>j0y msg hello", Some("N0JDS")).unwrap();
    let Some(Target::Relay(route)) = command.target else {
        panic!("expected partial relay route");
    };
    assert!(route.is_partial());
    assert_eq!(route.recipient(), "j0y");
    assert_eq!(route.hops().collect::<Vec<_>>(), ["N0JDS", "oh8stn", "j0y"]);
    assert_eq!(command.kind, CommandKind::Msg);

    assert!(parse_command(">OH8STN MSG HELLO", None).is_none());
    assert!(parsed("DR4CNK>OH8STN MSG HELLO").target.is_some());
    assert!(parse_command("DR4CNK>OH8STN>HELLO", None).is_none());
}

#[test]
fn explicit_and_implicit_targets_follow_reference_precedence() {
    assert!(parse_command("SNR?", None).is_none());
    assert!(parse_command("SNR?", Some("")).is_none());
    let implicit = parse_command("SNR?", Some("K1ABC")).unwrap();
    assert_eq!(implicit.target, Some(Target::Implicit("K1ABC")));
    assert_eq!(implicit.kind, CommandKind::SnrQuery);

    let explicit = parse_command("K2XYZ ACK", Some("K1ABC")).unwrap();
    assert_eq!(explicit.target, Some(Target::Call("K2XYZ")));
    assert_eq!(explicit.kind, CommandKind::Ack);

    assert!(parse_command("MSG HELLO", None).is_none());
    assert_eq!(
        parse_command("MSG HELLO", Some("K1ABC")).unwrap().payload,
        "HELLO"
    );
}

#[test]
fn only_heartbeat_forms_are_bare_commands() {
    let cq = parsed("CQ DX EM73");
    assert_eq!(cq.target, None);
    assert_eq!(cq.kind, CommandKind::Cq);
    assert_eq!(cq.arg, Some(CommandArg::Grid("EM73")));

    let hb = parsed("HB EM73");
    assert_eq!(hb.kind, CommandKind::Heartbeat);
    assert_eq!(hb.arg, Some(CommandArg::Grid("EM73")));

    let invalid_grid = parsed("CQ DX HELLO");
    assert_eq!(invalid_grid.arg, None);
    assert_eq!(invalid_grid.payload, "HELLO");

    assert!(parse_command("HEARTBEAT SNR", None).is_none());
    assert!(parse_command("INFO", None).is_none());
}

#[test]
fn longest_command_match_and_typed_arguments_match_reference() {
    let msgs = parsed("K1ABC QUERY MSGS?");
    assert_eq!(msgs.kind, CommandKind::QueryMsgs);
    assert_eq!(msgs.command, "QUERY MSGS?");

    let query_msg = parsed("K1ABC QUERY MSG 00032");
    assert_eq!(query_msg.kind, CommandKind::QueryMsg);
    assert_eq!(
        query_msg.arg,
        Some(CommandArg::MessageId {
            raw: "00032",
            value: 32
        })
    );
    assert_eq!(query_msg.payload, "");

    let invalid_id = parsed("K1ABC QUERY MSG nope");
    assert_eq!(invalid_id.kind, CommandKind::QueryMsg);
    assert_eq!(invalid_id.arg, None);
    assert_eq!(invalid_id.payload, "nope");

    let call = parsed("K1ABC QUERY CALL N0CALL?");
    assert_eq!(call.kind, CommandKind::QueryCall);
    assert_eq!(call.arg, Some(CommandArg::Call("N0CALL")));

    let to = parsed("K1ABC MSG TO:N0CALL HELLO");
    assert_eq!(to.kind, CommandKind::MsgTo);
    assert_eq!(to.arg, Some(CommandArg::Call("N0CALL")));
    assert_eq!(to.payload, "HELLO");

    assert_eq!(
        parsed("K1ABC MSG TO:@@A1 HELLO").arg,
        Some(CommandArg::Call("@@A1"))
    );
}

#[test]
fn validates_message_ids_and_all_reference_grid_lengths() {
    for grid in ["FN20", "FN20AB", "FN20AB12", "FN20AB12CD", "FN20AB12CD34"] {
        let text = format!("K1ABC GRID {grid}");
        assert_eq!(parsed(&text).arg, Some(CommandArg::Grid(grid)));
    }

    assert_eq!(
        parsed("K1ABC QUERY MSG 2147483647").arg,
        Some(CommandArg::MessageId {
            raw: "2147483647",
            value: i32::MAX
        })
    );
    assert_eq!(parsed("K1ABC QUERY MSG 2147483648").arg, None);
    assert_eq!(parsed("K1ABC GRID FN20AY").arg, None);
}

#[test]
fn preserves_question_and_colon_boundary_asymmetry() {
    assert!(parse_command("K1ABC STATUSX", None).is_none());

    let snr = parsed("K1ABC SNR?TAIL");
    assert_eq!(snr.kind, CommandKind::SnrQuery);
    assert_eq!(snr.payload, "TAIL");

    let compat = parsed("K1ABC ?");
    assert_eq!(compat.kind, CommandKind::SnrQuery);
    assert_eq!(compat.command, "?");
}

#[test]
fn rejects_non_messages_and_invalid_address_shapes() {
    assert!(parse_command("", None).is_none());
    assert!(parse_command("`K1ABC MSG", None).is_none());
    assert!(parse_command("HELLO BRAVE NEW WORLD", None).is_none());
    assert!(parse_command("K1ABC STATéé", None).is_none());
    assert!(parse_command("@GROUP: K1ABC MSG HELLO", None).is_none());
    assert!(parse_command(" K1ABC MSG HELLO", None).is_none());

    assert_eq!(parsed("K1ABC MSG héllo").payload, "héllo");
}

#[test]
fn command_wire_metadata_matches_js8call_tables() {
    assert_eq!(CommandKind::from_wire(" MSG"), Some(CommandKind::Msg));
    assert_eq!(CommandKind::from_wire("?"), Some(CommandKind::SnrQuery));
    assert_eq!(CommandKind::from_wire("  "), Some(CommandKind::FreeText));
    assert_eq!(CommandKind::from_wire("MSG"), None);
    assert_eq!(CommandKind::from_wire(" QUERY MSG"), None);
    assert_eq!(CommandKind::from_wire("  MSG"), None);
    assert_eq!(CommandKind::from_wire(" MSG "), None);
    assert_eq!(CommandKind::from_wire(" msg"), None);
    assert_eq!(
        CommandKind::from_token("query msg"),
        Some(CommandKind::QueryMsg)
    );
    assert_eq!(CommandKind::from_token("CQ DX"), Some(CommandKind::Cq));
    assert_eq!(CommandKind::Msg.wire_code(), Some(9));
    assert_eq!(CommandKind::QueryMsg.wire_code(), Some(11));
    assert_eq!(CommandKind::Cq.wire_code(), None);
    assert_eq!(CommandKind::Msg.wire_token(), Some(" MSG"));

    assert!(CommandKind::SnrQuery.is_autoreply());
    assert!(CommandKind::AgainQuery.is_autoreply());
    assert!(!CommandKind::Status.is_autoreply());
    assert!(CommandKind::Msg.is_buffered());
    assert!(CommandKind::Snr.is_buffered());
    assert!(CommandKind::Msg.is_buffered_code());
    assert!(!CommandKind::Snr.is_buffered_code());
    assert!(is_buffered_token(" SNR?"));
    assert!(is_buffered_token(">"));
    assert!(is_buffered_token(" CQ"));
    assert!(is_buffered_token(" HB"));
    assert!(is_buffered_token(" HEARTBEAT"));
    assert!(!is_buffered_token("?"));
    assert!(!is_buffered_token(" MSG "));
    assert_eq!(CommandKind::Msg.checksum_bits(), 16);
    assert_eq!(CommandKind::Grid.checksum_bits(), 0);
    assert!(CommandKind::HeartbeatSnr.has_snr());
}

#[test]
fn recognizes_every_reference_wire_token() {
    let cases = [
        (" HEARTBEAT", CommandKind::Heartbeat, None),
        (" HB", CommandKind::Heartbeat, None),
        (" CQ", CommandKind::Cq, None),
        (" SNR?", CommandKind::SnrQuery, Some(0)),
        ("?", CommandKind::SnrQuery, Some(0)),
        (" DIT DIT", CommandKind::DitDit, Some(1)),
        (" NACK", CommandKind::Nack, Some(2)),
        (" HEARING?", CommandKind::HearingQuery, Some(3)),
        (" GRID?", CommandKind::GridQuery, Some(4)),
        (">", CommandKind::Relay, Some(5)),
        (" STATUS?", CommandKind::StatusQuery, Some(6)),
        (" STATUS", CommandKind::Status, Some(7)),
        (" HEARING", CommandKind::Hearing, Some(8)),
        (" MSG", CommandKind::Msg, Some(9)),
        (" MSG TO:", CommandKind::MsgTo, Some(10)),
        (" QUERY", CommandKind::Query, Some(11)),
        (" QUERY MSGS", CommandKind::QueryMsgs, Some(12)),
        (" QUERY MSGS?", CommandKind::QueryMsgs, Some(12)),
        (" QUERY CALL", CommandKind::QueryCall, Some(13)),
        (" ACK", CommandKind::Ack, Some(14)),
        (" GRID", CommandKind::Grid, Some(15)),
        (" INFO?", CommandKind::InfoQuery, Some(16)),
        (" INFO", CommandKind::Info, Some(17)),
        (" FB", CommandKind::Fb, Some(18)),
        (" HW CPY?", CommandKind::HowCopyQuery, Some(19)),
        (" SK", CommandKind::Sk, Some(20)),
        (" RR", CommandKind::Rr, Some(21)),
        (" QSL?", CommandKind::QslQuery, Some(22)),
        (" QSL", CommandKind::Qsl, Some(23)),
        (" CMD", CommandKind::Cmd, Some(24)),
        (" SNR", CommandKind::Snr, Some(25)),
        (" NO", CommandKind::No, Some(26)),
        (" YES", CommandKind::Yes, Some(27)),
        (" 73", CommandKind::SeventyThree, Some(28)),
        (" HEARTBEAT SNR", CommandKind::HeartbeatSnr, Some(29)),
        (" AGN?", CommandKind::AgainQuery, Some(30)),
        (" ", CommandKind::FreeText, Some(31)),
        ("  ", CommandKind::FreeText, Some(31)),
    ];

    for (token, kind, code) in cases {
        let parsed = CommandKind::from_wire(token);
        assert_eq!(parsed, Some(kind), "failed to parse {token:?}");
        assert_eq!(parsed.and_then(CommandKind::wire_code), code);
    }
}

#[test]
fn recognizes_every_reference_command_token() {
    let cases = [
        ("SNR?", CommandKind::SnrQuery),
        ("DIT DIT", CommandKind::DitDit),
        ("NACK", CommandKind::Nack),
        ("HEARING?", CommandKind::HearingQuery),
        ("GRID?", CommandKind::GridQuery),
        ("STATUS?", CommandKind::StatusQuery),
        ("STATUS", CommandKind::Status),
        ("HEARING", CommandKind::Hearing),
        ("MSG", CommandKind::Msg),
        ("MSG TO:", CommandKind::MsgTo),
        ("QUERY", CommandKind::Query),
        ("QUERY MSGS", CommandKind::QueryMsgs),
        ("QUERY MSGS?", CommandKind::QueryMsgs),
        ("QUERY CALL", CommandKind::QueryCall),
        ("ACK", CommandKind::Ack),
        ("GRID", CommandKind::Grid),
        ("INFO?", CommandKind::InfoQuery),
        ("INFO", CommandKind::Info),
        ("FB", CommandKind::Fb),
        ("HW CPY?", CommandKind::HowCopyQuery),
        ("SK", CommandKind::Sk),
        ("RR", CommandKind::Rr),
        ("QSL?", CommandKind::QslQuery),
        ("QSL", CommandKind::Qsl),
        ("CMD", CommandKind::Cmd),
        ("SNR", CommandKind::Snr),
        ("NO", CommandKind::No),
        ("YES", CommandKind::Yes),
        ("73", CommandKind::SeventyThree),
        ("HEARTBEAT SNR", CommandKind::HeartbeatSnr),
        ("AGN?", CommandKind::AgainQuery),
    ];

    for (token, kind) in cases {
        let text = format!("K1ABC {token}");
        assert_eq!(parsed(&text).kind, kind, "failed to parse {token:?}");
    }

    for token in [
        "CQ CQ CQ",
        "CQ CONTEST",
        "CQ FIELD",
        "CQ DX",
        "CQ QRP",
        "CQ FD",
        "CQ CQ",
        "CQ",
    ] {
        assert_eq!(
            parsed(token).kind,
            CommandKind::Cq,
            "failed to parse {token:?}"
        );
    }
    for token in ["HB", "HEARTBEAT"] {
        assert_eq!(
            parsed(token).kind,
            CommandKind::Heartbeat,
            "failed to parse {token:?}"
        );
    }
}
