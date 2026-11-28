use crate::{
    composition::{self, Outcome},
    composition_source::{
        NewsObservation, SourceObservation, SourceRequest, replay_news, retain_news,
    },
    core::{Engine, OutputCheckpoint, base_result},
    input,
    news::{NewsAction, NewsFetch, NewsWindow},
    news_source::NewsPublisher,
    ports::{self, NewsWebFormat},
    scheduler::Scheduler,
    store::Store,
};
use minidora_domain::{
    BranchState, EffectPhase, EvidencePacket, IndependentRequest, InputBundle, Intent, Lifecycle,
    NewsPeriod, RequestEnvelope, RetryAfterDirective, RetryAfterObservation, digest, identity,
};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

const CAPTURED_AT: u64 = 1_795_752_000_000;
const PUBLISHED: &str = "2026-11-27T12:51:49+09:00";
const ARTICLE: &str = "https://www.sankei.com/article/20261127-ABCDEF123456ABCDEF123456/";
const OTHER_ARTICLE: &str = "https://www.sankei.com/article/20261127-ABCDEG123456ABCDEF123456/";

fn bundle(store: &Store, case: &str) -> InputBundle {
    seal_bundle(
        store,
        case,
        "- 直近24時間のニュースを教えて\n- 直近24時間のニュースを教えて\n- 7+2",
    )
}

fn seal_bundle(store: &Store, case: &str, text: &str) -> InputBundle {
    let mut request = RequestEnvelope::new(case, "authored", "alice", text);
    request
        .permissions
        .read_hosts
        .insert(NewsPublisher::Sankei.host().into());
    assert!(
        store
            .register_input("alice", &request, &identity(&request).unwrap())
            .unwrap()
    );
    let sealed = input::seal(&request, None).unwrap();
    assert_eq!(
        sealed.meaning.parser_version,
        crate::composition::NEWS_PARSER_VERSION
    );
    sealed
}

fn items(bundle: &InputBundle) -> &[IndependentRequest] {
    let Intent::IndependentRequests { items } = &bundle.meaning.intent else {
        panic!("expected the authored list");
    };
    items
}

// These are in-memory responses to scripted requests. No transport is invoked.
fn packet(uri: &str, format: NewsWebFormat, text: &str) -> EvidencePacket {
    let (kind, parser) = match format {
        NewsWebFormat::ListingHtml => ("NEWS_LISTING_HTML", "strict-utf8-news-listing-html-v1"),
        NewsWebFormat::ArticleHtml => ("NEWS_ARTICLE_HTML", "strict-utf8-news-article-html-v1"),
        NewsWebFormat::ListingXml => panic!("this fixture only configures Sankei"),
    };
    let mut packet = EvidencePacket {
        id: String::new(),
        mode: "LIVE".into(),
        packet_kind: kind.into(),
        request_uri: uri.into(),
        final_uri: uri.into(),
        redirect_chain: vec![uri.into()],
        observed_at_unix_ms: CAPTURED_AT,
        provider_id: "https-news-read-v1".into(),
        http_status: 200,
        raw_body: text.as_bytes().to_vec(),
        raw_sha256: digest(text.as_bytes()),
        extracted_text: Some(text.into()),
        parser_version: parser.into(),
        independence_group: uri.into(),
        failure: None,
        retry_after: None,
    };
    packet.id = ports::fetch_packet_identity(&packet).unwrap();
    packet
}

fn article(uri: &str, published: &str) -> EvidencePacket {
    let html = format!(
        r#"<script type="application/ld+json">[{{"@type":"NewsArticle","url":"{uri}","headline":"観測塔の新しい照明","datePublished":"{published}"}},{{"@type":"BreadcrumbList"}}]</script><article class="article-wrapper free"><div class="article-body clearfix "><p class="article-text ">古い観測塔の照明を冬の運用に合わせて調整しました。</p></div></article>"#
    );
    packet(uri, NewsWebFormat::ArticleHtml, &html)
}

fn listing(uris: &[&str]) -> EvidencePacket {
    let cards: String = uris
        .iter()
        .map(|uri| format!(r#"<article class="storycard"><a href="{uri}">story</a></article>"#))
        .collect();
    let html = format!(r#"<main class="main-column">{cards}</main>"#);
    packet(
        NewsPublisher::Sankei.listing_url(),
        NewsWebFormat::ListingHtml,
        &html,
    )
}

fn retry_packet(
    mut packet: EvidencePacket,
    status: u16,
    code: &str,
    retry_after: RetryAfterObservation,
) -> EvidencePacket {
    packet.packet_kind = "FETCH_FAILURE".into();
    packet.http_status = status;
    packet.raw_body.clear();
    packet.raw_sha256 = digest(&[]);
    packet.extracted_text = None;
    packet.failure = Some(code.into());
    packet.observed_at_unix_ms = retry_after.received_at_unix_ms;
    packet.retry_after = Some(retry_after);
    packet.id = ports::fetch_packet_identity(&packet).unwrap();
    packet
}

fn rate_limited() -> EvidencePacket {
    retry_packet(
        article(ARTICLE, PUBLISHED),
        429,
        "HTTP_RATE_LIMITED",
        RetryAfterObservation {
            directive: RetryAfterDirective::DelaySeconds { seconds: 60 },
            received_at_unix_ms: CAPTURED_AT,
            not_before_unix_ms: Some(CAPTURED_AT + 60_000),
        },
    )
}

fn scripted(
    store: &Store,
    bundle: &InputBundle,
    item: &IndependentRequest,
    action: &NewsAction<'_>,
    article: EvidencePacket,
) -> minidora_domain::Result<NewsObservation> {
    let publisher = NewsPublisher::Sankei;
    let listing_uri = publisher.listing_url();
    let mut script = [
        (listing_uri, NewsWebFormat::ListingHtml, listing(&[ARTICLE])),
        (ARTICLE, NewsWebFormat::ArticleHtml, article),
    ]
    .into_iter();
    assert_eq!(action.period, NewsPeriod::Latest);
    let window = NewsWindow::at(action.period, CAPTURED_AT as i64).unwrap();
    let result = retain_news(
        store,
        bundle,
        item,
        action,
        window,
        |uri, format, permissions| {
            let (expected_uri, expected_format, packet) = script
                .next()
                .expect("unexpected request from the news action");
            assert_eq!(uri, expected_uri);
            assert_eq!(format, expected_format);
            assert_eq!(*permissions, publisher.read_permissions());
            let reference =
                store.put_artifact(&bundle.request.actor_id, &bundle.request.case_id, &packet)?;
            Ok(NewsFetch {
                packet,
                reference,
                redirect_wait: None,
            })
        },
        64_000,
    );
    assert!(script.next().is_none());
    result
}

fn current_news_packets(with_resolved_wait: bool) -> Vec<EvidencePacket> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let published = now.as_secs() - 3_600;
    let mut days = published / 86_400;
    let mut year = 1970;
    let leap = |year| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    while days >= 365 + u64::from(leap(year)) {
        days -= 365 + u64::from(leap(year));
        year += 1;
    }
    let month_days = [
        31,
        28 + u64::from(leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for length in month_days {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }
    let day = days + 1;
    let (hour, minute, second) = (published / 3600 % 24, published / 60 % 60, published % 60);
    let timestamp = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z");
    let uri = format!(
        "https://www.sankei.com/article/{year:04}{month:02}{day:02}-ABCDEF123456ABCDEF123456/"
    );
    let observed = u64::try_from(now.as_millis()).unwrap() + 60_000;
    let mut packets = if with_resolved_wait {
        let first = format!(
            "https://www.sankei.com/article/{year:04}{month:02}{day:02}-ABCDEG123456ABCDEF123456/"
        );
        vec![
            listing(&[&first, &uri]),
            retry_packet(
                article(&first, &timestamp),
                503,
                "HTTP_STATUS_FAILURE",
                RetryAfterObservation {
                    directive: RetryAfterDirective::DelaySeconds { seconds: 0 },
                    received_at_unix_ms: observed,
                    not_before_unix_ms: Some(observed + 1_000),
                },
            ),
        ]
    } else {
        vec![listing(&[&uri])]
    };
    packets.push(article(&uri, &timestamp));
    for (index, packet) in packets.iter_mut().enumerate() {
        packet.observed_at_unix_ms = observed
            + if with_resolved_wait && index == 2 {
                1_001
            } else {
                0
            };
        packet.id = ports::fetch_packet_identity(packet).unwrap();
    }
    packets
}

#[test]
fn news_file_reconciliation_replays_the_original_observation_without_resending() {
    for (case, text, locale) in [
        ("ordinary", "Show me the latest news", "en-US"),
        ("compound", "- 直近24時間のニュースを教えて\n- 7+2", "ja"),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let state = temporary.path().join("state");
        let target = temporary.path().join("news.txt");
        let mut request = RequestEnvelope::new(case, "news", "alice", text);
        request.locale = locale.into();
        request.output_path = Some(target.clone());
        request.permissions.write_paths.insert(target.clone());
        request
            .permissions
            .read_hosts
            .insert(NewsPublisher::Sankei.host().into());
        let mut engine = Engine::open(&state).unwrap();
        engine.expire_output_at = Some(OutputCheckpoint::Commit);
        let packets = current_news_packets(case == "ordinary");
        let expected_evidence: Vec<_> = packets
            .iter()
            .filter(|packet| packet.retry_after.is_some())
            .cloned()
            .collect();
        assert_eq!(expected_evidence.len(), usize::from(case == "ordinary"));
        let stopped =
            ports::with_test_news_packets(packets, || engine.execute("alice", request).unwrap());
        assert_eq!(stopped.lifecycle, Lifecycle::Suspend, "{case}: {stopped:?}");
        assert!(stopped.reason.as_deref().unwrap().starts_with("DEADLINE:"));
        assert_eq!(stopped.content_plan.as_ref().unwrap().locale, locale);
        assert_eq!(stopped.evidence, expected_evidence);
        assert!(
            stopped.text.contains("観測塔の新しい照明"),
            "{case}: {stopped:?}"
        );
        if case == "compound" {
            assert!(stopped.text.ends_with(" = 9"), "{stopped:?}");
        }
        let bytes = fs::read(&target).unwrap();
        assert_eq!(bytes, stopped.text.as_bytes());
        let modified = fs::metadata(&target).unwrap().modified().unwrap();
        let actions = engine.store.actions("alice", case).unwrap();
        let [action] = actions.as_slice() else {
            panic!("expected exactly one local action");
        };
        let [receipt] = stopped.receipts.as_slice() else {
            panic!("expected the original write receipt");
        };
        assert_eq!(action.phase, EffectPhase::Confirmed);
        assert_eq!(receipt.phase, EffectPhase::Confirmed);
        assert_eq!(action.action_id, receipt.action_id);
        drop(engine);

        let mut reopened = Engine::open(&state).unwrap();
        let recovered = ports::with_test_news_packets(Vec::new(), || {
            reopened.reconcile("alice", case).unwrap()
        });
        assert_eq!(
            recovered.lifecycle,
            Lifecycle::Commit,
            "{case}: {recovered:?}"
        );
        assert_eq!(recovered.dependencies, stopped.dependencies);
        assert_eq!(recovered.branch_outcomes, stopped.branch_outcomes);
        assert_eq!(recovered.evidence, stopped.evidence);
        assert_eq!(recovered.receipts, stopped.receipts);
        assert_eq!(recovered.execution_graph_ref, stopped.execution_graph_ref);
        assert_eq!(reopened.store.actions("alice", case).unwrap(), actions);
        assert_eq!(fs::read(&target).unwrap(), bytes);
        assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), modified);
    }
}

#[test]
fn compound_news_replays_the_same_packets_only_for_the_authored_item() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let request = bundle(&store, "origin");
    let [item, same_text_elsewhere, _] = items(&request) else {
        panic!("expected two news requests and one arithmetic item");
    };
    let action = NewsAction::bind(&item.intent).unwrap();
    let original = scripted(&store, &request, item, &action, article(ARTICLE, PUBLISHED)).unwrap();
    assert!(original.confirmed && original.complete);
    assert!(original.units.iter().any(|unit| unit.contains(ARTICLE)));
    assert!(original.retry_evidence.is_empty());

    let replay = replay_news(&store, &request, item, &action, &original.reference).unwrap();
    assert_eq!(replay.report_bytes, original.report_bytes);
    assert_eq!(replay.units, original.units);
    assert_eq!(replay.reference, original.reference);
    assert!(replay.confirmed && replay.complete);

    assert_ne!(item.source.start, same_text_elsewhere.source.start);
    let other_action = NewsAction::bind(&same_text_elsewhere.intent).unwrap();
    assert!(
        replay_news(
            &store,
            &request,
            same_text_elsewhere,
            &other_action,
            &original.reference,
        )
        .is_err()
    );
    let foreign = bundle(&store, "elsewhere");
    let foreign_item = &items(&foreign)[0];
    let foreign_action = NewsAction::bind(&foreign_item.intent).unwrap();
    assert!(
        replay_news(
            &store,
            &foreign,
            foreign_item,
            &foreign_action,
            &original.reference,
        )
        .is_err()
    );
}

#[test]
fn compound_news_never_confirms_an_altered_article_route_or_stale_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let request = bundle(&store, "origin");
    let item = &items(&request)[0];
    let action = NewsAction::bind(&item.intent).unwrap();
    let saved_packet = article(ARTICLE, PUBLISHED);
    let saved_reference = store
        .put_artifact("alice", "origin", &saved_packet)
        .unwrap();
    let original = scripted(&store, &request, item, &action, saved_packet).unwrap();
    let other_packet = article(OTHER_ARTICLE, PUBLISHED);
    let other_reference = store
        .put_artifact("alice", "origin", &other_packet)
        .unwrap();
    let retained = String::from_utf8(
        store
            .get_bytes("alice", "origin", &original.reference)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(retained.matches(&saved_reference).count(), 1);
    let altered = retained.replace(&saved_reference, &other_reference);
    let altered_reference = store
        .put_bytes("alice", "origin", altered.as_bytes())
        .unwrap();
    assert!(replay_news(&store, &request, item, &action, &altered_reference).is_err());

    let stale = scripted(
        &store,
        &request,
        item,
        &action,
        article(ARTICLE, "2026-11-25T12:51:49+09:00"),
    )
    .unwrap();
    assert!(!stale.confirmed && !stale.complete);
    let replay = replay_news(&store, &request, item, &action, &stale.reference).unwrap();
    assert!(!replay.confirmed && !replay.complete);
    assert_eq!(replay.report_bytes, stale.report_bytes);
    assert!(
        std::str::from_utf8(&replay.report_bytes)
            .unwrap()
            .contains("NEWS_PUBLICATION_OUTSIDE_WINDOW")
    );
}

#[test]
fn compound_news_replays_the_wait_for_a_listed_article() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let request = seal_bundle(&store, "origin", "- 直近24時間のニュースを教えて\n- 7+2");
    let items = items(&request);
    let wait = rate_limited();
    let mut source_starts = Vec::new();
    let mut graph = Scheduler::new(
        composition::nodes(items).collect(),
        request.request.budget.max_actions,
    )
    .unwrap();
    let mut outcome = Outcome::new();
    composition::evaluate_items(
        &store,
        &request,
        items,
        &mut graph,
        None,
        |index, source| {
            let SourceRequest::News { item, action } = source else {
                panic!("only the news item can start a read");
            };
            source_starts.push(index);
            assert_eq!(item.source, items[0].source);
            scripted(&store, &request, item, &action, wait.clone()).map(SourceObservation::News)
        },
        |_, bytes| store.put_bytes("alice", "origin", bytes),
        &mut outcome,
    )
    .unwrap();
    assert_eq!(source_starts, [0]);
    let [news, arithmetic] = outcome.branches.as_slice() else {
        panic!("expected the news and arithmetic outcomes");
    };
    assert_eq!(news.state, BranchState::Blocked);
    assert_eq!(
        news.reason_code.as_deref(),
        Some("NEWS_ARTICLES_UNAVAILABLE")
    );
    assert_eq!(arithmetic.state, BranchState::Confirmed);
    assert!(outcome.units.iter().any(|unit| unit.ends_with(" = 9")));
    assert_eq!(outcome.retry_evidence, [wait.clone()]);

    let mut retained = base_result(&request.request);
    retained.lifecycle = Lifecycle::Suspend;
    retained.dependencies = outcome
        .branches
        .iter()
        .map(|branch| branch.artifact_ref.clone())
        .collect();
    retained.branch_outcomes = outcome.branches;
    retained.evidence = outcome.retry_evidence;
    composition::verify_retry_source(&store, &request, &retained, &wait).unwrap();

    let mut altered = retained.clone();
    let mut changed_wait = wait.clone();
    let retry_after = changed_wait.retry_after.as_mut().unwrap();
    retry_after.directive = RetryAfterDirective::DelaySeconds { seconds: 90 };
    retry_after.not_before_unix_ms = Some(CAPTURED_AT + 90_000);
    changed_wait.id = ports::fetch_packet_identity(&changed_wait).unwrap();
    altered.evidence = vec![changed_wait.clone()];
    assert!(composition::verify_retry_source(&store, &request, &altered, &changed_wait).is_err());

    let mut wrong_branch = retained;
    let arithmetic_ref = wrong_branch.branch_outcomes[1].artifact_ref.clone();
    wrong_branch.branch_outcomes[0].artifact_ref = arithmetic_ref;
    assert!(composition::verify_retry_source(&store, &request, &wrong_branch, &wait).is_err());
}
