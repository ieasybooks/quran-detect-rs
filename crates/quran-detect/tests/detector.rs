//! Public behavior on the embedded Tanzil corpus, ported from the reference test suite.

use quran_detect::{
    Brackets, Config, Detector, EditKind, Error, ErrorKind, ExactContext, Formatting, Match, Plan,
    Preset, Script, VerseRef,
};

/// Normalized words without vowel, tajweed or pause marks, for comparing wording.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| {
                    let point = u32::from(*c);
                    !((0x064B..=0x0652).contains(&point)
                        || point == 0x0670
                        || (0x06D6..=0x06ED).contains(&point)
                        || *c == 'ـ')
                })
                .map(|c| match c {
                    'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
                    'ة' => 'ه',
                    'ى' => 'ي',
                    other => other,
                })
                .filter(|c| ('\u{0621}'..='\u{064A}').contains(c))
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Identify, plan, apply and revert, checking the invariants every result must satisfy.
fn checked(detector: &Detector, text: &str) -> (String, Vec<Match>) {
    let config = detector.config();
    let matches = detector.identify(text);
    let plan = detector
        .plan_matches(text, &matches, Formatting::default())
        .unwrap();
    let plan: Plan = serde_json::from_str(&serde_json::to_string(&plan).unwrap()).unwrap();
    let output = plan.apply(text).unwrap();
    assert_eq!(output, detector.replace(text, Formatting::default()));
    assert_eq!(plan.revert(&output).unwrap(), text);
    let mut cursor = 0;
    for m in &matches {
        assert!(cursor <= m.start && m.start < m.end);
        cursor = m.end;
        assert_eq!(&text[m.start..m.end], m.text);
        assert!(m.recommended);
        assert!(m.alignment_errors <= usize::try_from(config.max_edits()).unwrap());
        assert!(m.error_fraction <= config.error_fraction());
        assert_eq!(m.edition.script, detector.script());
        for edit in &m.edits {
            assert!(m.start <= edit.start && edit.start <= edit.end && edit.end <= m.end);
            assert_eq!(&text[edit.start..edit.end], edit.before);
            for kind in &edit.kinds {
                let allowed = match kind {
                    EditKind::Exact => true,
                    EditKind::Typo => config.allowed_errors().contains(ErrorKind::Typo),
                    EditKind::Missing => config.allowed_errors().contains(ErrorKind::Missing),
                    EditKind::Extra => config.allowed_errors().contains(ErrorKind::Extra),
                    EditKind::Join => config.allowed_errors().contains(ErrorKind::Join),
                    EditKind::Split => config.allowed_errors().contains(ErrorKind::Split),
                    EditKind::Substitution => {
                        config.allowed_errors().contains(ErrorKind::Substitution)
                    }
                    EditKind::Bracket | EditKind::Reference => false,
                };
                assert!(allowed, "{kind:?} in {text:?}");
            }
        }
    }
    assert!(plan.proposals.iter().all(|p| p.id > 0));
    (output, matches)
}

fn restored(
    detector: &Detector,
    text: &str,
    matches: Option<&[Match]>,
    formatting: Formatting,
) -> (String, Plan) {
    let plan = match matches {
        Some(matches) => detector.plan_matches(text, matches, formatting).unwrap(),
        None => detector.plan(text, formatting),
    };
    let output = match matches {
        Some(matches) => detector.replace_matches(text, matches, formatting).unwrap(),
        None => detector.replace(text, formatting),
    };
    let round_trip: Plan = serde_json::from_str(&serde_json::to_string(&plan).unwrap()).unwrap();
    assert_eq!(round_trip.apply(text).unwrap(), output);
    assert_eq!(plan.revert(&output).unwrap(), text);
    (output, plan)
}

fn format(append_reference: bool, brackets: Brackets) -> Formatting {
    Formatting {
        append_reference,
        brackets,
    }
}

#[test]
fn detectors_are_shareable_between_threads() {
    fn shareable<T: Send + Sync + Clone + 'static>() {}
    shareable::<Detector>();
    let detector = Detector::default();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let detector = detector.clone();
            std::thread::spawn(move || detector.replace("قل هو الله أحد", Formatting::default()))
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), "قُلْ هُوَ اللَّهُ أَحَدٌ");
    }
}

#[test]
fn attached_prefixes_stay_outside_the_match() {
    for preset in Preset::ALL {
        let detector = Detector::new(Config::preset(preset));
        for prefix in ["ب", "بِ", "و", "ف", "ك", "ل", "وب", "فب", "وَبِ"] {
            let lead = format!("فيختم {prefix}");
            let text = format!("{lead}قل هو الله أحد");
            let (output, matches) = checked(&detector, &text);
            assert_eq!(matches.len(), 1, "{preset} {prefix}");
            assert_eq!(matches[0].start, lead.len(), "{preset} {prefix}");
            assert!(output.starts_with(&format!("{lead}قُلْ")), "{output}");
        }
    }
}

#[test]
fn balanced_restores_common_quotations() {
    let detector = Detector::default();
    for (text, target) in [
        ("الحمد لله رب العالمين، وبعد…", "الحمد لله رب العالمين"),
        ("بسم الله الرحمن الرحيم", "بسم الله الرحمن الرحيم"),
        ("يكاد البرق يخطف أبصرهم", "يكاد البرق يخطف أبصارهم"),
        ("قال أنا عبد الله المبارك", "قال إني عبد الله"),
        (
            "الله لا إله إلا هو سبحانه الحي القيوم",
            "الله لا إله إلا هو الحي القيوم",
        ),
        ("الله لا إله إلا الحي القيوم", "الله لا إله إلا هو الحي القيوم"),
    ] {
        let (_, matches) = checked(&detector, text);
        assert!(!matches.is_empty(), "{text}");
        assert_eq!(words(&matches[0].canonical_text), words(target), "{text}");
    }
}

#[test]
fn the_error_fraction_is_a_real_boundary() {
    let text = "يكاد البرق يخطف أبصرهم";
    let low = Detector::new(Config::default().with_error_fraction(0.24).unwrap());
    let high = Detector::new(Config::default().with_error_fraction(0.25).unwrap());
    assert!(checked(&low, text).0.contains("أبصرهم"));
    assert!(checked(&high, text).0.contains("أَبْصَارَهُمْ"));
}

#[test]
fn minimum_canonical_length() {
    let short = Detector::new(
        Config::default()
            .with_min_words(2)
            .unwrap()
            .with_max_edits(0),
    );
    assert_eq!(checked(&short, "رب العالمين").1[0].text, "رب العالمين");
    let long = Detector::new(Config::default().with_min_words(5).unwrap());
    assert_eq!(checked(&long, "قل هو الله أحد").1, []);
}

#[test]
fn offsets_survive_markup_entities_ligatures_and_line_endings() {
    let detector = Detector::default();
    for text in [
        "🙂\r\n﴿بِقل هو الله أحد﴾\r\nالنهاية",
        "بِقل هو الله أحد",
        "&#1576;قل هو الله أحد",
        "بـقل هو الله أحد",
        "﴿﷽﴾",
        "بقل هو الله أحد\u{200F}",
        "﴿الله لا إله إلا هو﴾. ثم: ﴿يكاد البرق يخطف أبصرهم﴾",
        "<p>قال تعالى: <b>قل هو الله أحد</b></p>",
        "",
        " \n",
    ] {
        checked(&detector, text);
    }
    // Decomposed text, as CPython's NFD would produce it.
    checked(&detector, "ب\u{0650}قل هو الله ا\u{0654}حد");
}

#[test]
fn a_canonical_leading_conjunction_is_not_cut() {
    let (output, matches) = checked(&Detector::default(), "والعصر إن الإنسان لفي خسر");
    assert_eq!(matches[0].start, 0);
    assert!(output.starts_with("وَالْعَصْرِ"));
}

#[test]
fn repeated_verses_report_tied_references() {
    let (_, matches) = checked(&Detector::default(), "فبأي آلاء ربكما تكذبان");
    assert_eq!(matches.len(), 1);
    assert!(matches[0].equally_ranked_targets >= 31);
    assert!(matches[0].reference_alternatives.len() >= 31);
    assert_eq!(matches[0].references, [VerseRef(55, 13)]);
}

#[test]
fn stale_and_unordered_matches_are_rejected() {
    let detector = Detector::default();
    let source = "قال: الله لا إله إلا هو الحي القيوم، ثم قل هو الله أحد";
    let candidates = detector.alternatives(source);
    let recommended: Vec<Match> = candidates
        .iter()
        .filter(|c| c.recommended)
        .cloned()
        .collect();
    assert_eq!(recommended, detector.identify(source));
    assert!(candidates.len() > recommended.len());
    assert_eq!(
        detector
            .replace_matches(source, &recommended, Formatting::default())
            .unwrap(),
        detector.replace(source, Formatting::default())
    );
    assert_eq!(
        detector
            .replace_matches(source, &[], Formatting::default())
            .unwrap(),
        source
    );
    let chosen = &candidates[0];
    let (_, plan) = restored(
        &detector,
        source,
        Some(std::slice::from_ref(chosen)),
        Formatting::default(),
    );
    assert_eq!(plan.proposals.len(), 1);
    assert_eq!(plan.proposals[0].candidate_id, chosen.id);
    assert_eq!(
        detector.replace_matches(source, &candidates, Formatting::default()),
        Err(Error::StaleMatches)
    );
    let changed = format!("{source} ");
    assert_eq!(
        detector.replace_matches(&changed, &recommended, Formatting::default()),
        Err(Error::StaleMatches)
    );
    let reversed: Vec<Match> = recommended.iter().rev().cloned().collect();
    assert_eq!(
        detector.replace_matches(source, &reversed, Formatting::default()),
        Err(Error::StaleMatches)
    );
    let simple = Detector::default().with_script(Script::Simple);
    assert!(matches!(
        simple.replace_matches(source, &recommended, Formatting::default()),
        Err(Error::EditionMismatch { index: 1, .. })
    ));
}

#[test]
fn reference_choices_resolve_coordinates_and_citation_together() {
    let detector = Detector::default();
    let source = "«الله لا إله إلا هو الحي القيوم»";
    let mut matched = detector.identify(source).remove(0);
    let alternatives = matched.reference_alternatives.clone();
    let paths: Vec<&[VerseRef]> = alternatives
        .iter()
        .map(|r| r.references.as_slice())
        .collect();
    assert!(paths.contains(&&[VerseRef(2, 255)][..]) && paths.contains(&&[VerseRef(3, 2)][..]));
    let selected = alternatives
        .iter()
        .find(|r| r.references == [VerseRef(3, 2)])
        .unwrap();
    matched.reference_choice = Some(selected.id.clone());
    let (output, plan) = restored(
        &detector,
        source,
        Some(std::slice::from_ref(&matched)),
        format(true, Brackets::Braces),
    );
    assert!(output.ends_with("} [آل عمران - ٢]"), "{output}");
    let proposal = &plan.proposals[0];
    assert_eq!(proposal.references, [VerseRef(3, 2)]);
    assert_eq!(proposal.reference_start, selected.reference_start);
    assert_eq!(proposal.reference_end, selected.reference_end);
    assert_eq!(proposal.candidate_id, matched.id);
    let mut bad = matched.clone();
    bad.reference_choice = Some("not-a-member".into());
    assert_eq!(
        detector.replace_matches(source, &[bad], Formatting::default()),
        Err(Error::UnknownReferenceChoice(1))
    );
    let mut bad = matched.clone();
    for alternative in &mut bad.reference_alternatives {
        if alternative.id == selected.id {
            alternative.reference_start += 1;
        }
    }
    assert_eq!(
        detector.replace_matches(source, &[bad], Formatting::default()),
        Err(Error::InvalidReferenceCoordinates(1))
    );
}

#[test]
fn every_formatting_combination_is_reversible_and_idempotent() {
    let detector = Detector::default();
    let source = "قال: « \r\nالله لا إله إلا الحي القيوم\t »، ثم أكمل.";
    let matches = detector.identify(source);
    let canonical = &matches[0].canonical_text;
    for append in [false, true] {
        for brackets in Brackets::ALL {
            let options = format(append, brackets);
            let normalized = brackets != Brackets::Keep;
            let (left, right) = if normalized { ("{", "}") } else { ("«", "»") };
            let (pad_left, pad_right) = if brackets == Brackets::BracesTrimmed {
                ("", "")
            } else {
                (" \r\n", "\t ")
            };
            let citation = if append {
                " [البقرة - ٢٥٥]"
            } else {
                ""
            };
            let (inside, outside) = if normalized {
                ("", citation)
            } else {
                (citation, "")
            };
            let expected = format!(
                "قال: {left}{pad_left}{canonical}{inside}{pad_right}{right}{outside}، ثم أكمل."
            );
            let (output, plan) = restored(&detector, source, Some(&matches), options);
            assert_eq!(output, expected);
            assert_eq!(detector.replace(&output, options), output);
            assert_eq!(plan.proposals[0].start, matches[0].start);
            assert_eq!(plan.proposals[0].end, matches[0].end);
            assert_eq!(
                detector.replace_matches(source, &[], options).unwrap(),
                source
            );
        }
    }
}

#[test]
fn recognized_bracket_pairs_become_braces() {
    let detector = Detector::default();
    for (open, close) in [
        ("«", "»"),
        ("“", "”"),
        ("(", ")"),
        ("[", "]"),
        ("﴿", "﴾"),
        ("{", "}"),
    ] {
        let source = format!("{open}قل هو الله أحد{close}");
        let (output, _) = restored(&detector, &source, None, format(false, Brackets::Braces));
        assert!(output.starts_with('{') && output.ends_with('}'), "{output}");
    }
}

#[test]
fn wrappers_that_do_not_enclose_exactly_the_quotation_are_preserved() {
    let detector = Detector::default();
    for source in [
        "«قل هو الله أحد)",
        "«قل هو الله أحد،»",
        "«بقل هو الله أحد»",
        "«قال قل هو الله أحد»",
        "قل هو الله أحد",
    ] {
        let plain = detector.replace(source, Formatting::default());
        let (output, _) = restored(
            &detector,
            source,
            None,
            format(false, Brackets::BracesTrimmed),
        );
        assert_eq!(output, plain, "{source}");
    }
}

#[test]
fn nearest_pairs_shared_wrappers_and_verse_ranges() {
    let detector = Detector::default();
    let (output, _) = restored(
        &detector,
        "«(قل هو الله أحد)»",
        None,
        format(false, Brackets::Braces),
    );
    assert!(
        output.starts_with("«{") && output.ends_with("}»"),
        "{output}"
    );
    let source = "«قل هو الله أحد، ثم الله لا إله إلا هو الحي القيوم»";
    let (output, _) = restored(&detector, source, None, format(true, Brackets::Braces));
    assert!(output.starts_with('«') && output.ends_with('»'), "{output}");
    assert!(output.contains("[الإخلاص - ١]") && output.contains("[البقرة - ٢٥٥]"));
    let (output, _) = restored(
        &detector,
        "﴿قل هو الله أحد الله الصمد﴾",
        None,
        format(true, Brackets::Braces),
    );
    assert!(output.ends_with("} [الإخلاص - ١-٢]"), "{output}");
}

#[test]
fn conflicting_existing_citations_are_kept_and_reported() {
    let detector = Detector::default();
    for source in [
        "{قل هو الله أحد} [الفاتحة - ١]",
        "«قل هو الله أحد [الفاتحة - ١]»",
    ] {
        let (output, plan) = restored(&detector, source, None, format(true, Brackets::Braces));
        assert!(output.contains("[الفاتحة - ١]") && !output.contains("[الإخلاص - ١]"));
        let conflicts = &plan.proposals[0].formatting_conflicts;
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].expected, "[الإخلاص - ١]");
        assert_eq!(
            &source[conflicts[0].start..conflicts[0].end],
            "[الفاتحة - ١]"
        );
    }
    let (output, _) = restored(
        &detector,
        "{قل هو الله أحد} [الإخلاص - ١]",
        None,
        format(true, Brackets::Braces),
    );
    assert_eq!(output.matches("[الإخلاص - ١]").count(), 1);
}

#[test]
fn context_guards_keep_supported_repairs_and_block_neighbor_words() {
    let detector = Detector::default();
    for (source, target) in [
        ("الله لا إله إلا الحي القيوم", "الله لا إله إلا هو الحي القيوم"),
        ("يكاد البرق يخطف أبصرهم", "يكاد البرق يخطف أبصارهم"),
    ] {
        assert_eq!(words(&checked(&detector, source).0), words(target));
    }
    let source = "قال ربي ان ابني من اهلي وان وعدك الحق وانت احكم الحاكمين قال انه ليس من اهلك";
    let guarded = words(&checked(&detector, source).0).join(" ");
    assert!(
        !guarded.contains("يا نوح") && guarded.contains("قال انه ليس من اهلك"),
        "{guarded}"
    );
    let legacy = Detector::new(Config::default().with_min_exact_context(ExactContext::both(0)));
    assert!(
        words(&checked(&legacy, source).0)
            .join(" ")
            .contains("يا نوح")
    );
}

#[test]
fn scripts_change_rendering_but_not_detection() {
    let source = "أولئك على هدى من ربهم وأولئك هم المفلحون";
    let plain = Detector::default();
    let simple = Detector::default().with_script(Script::Simple);
    let (plain_output, plain_matches) = checked(&plain, source);
    let (simple_output, simple_matches) = checked(&simple, source);
    let spans = |matches: &[Match]| {
        matches
            .iter()
            .map(|m| (m.start, m.end, m.references.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(spans(&plain_matches), spans(&simple_matches));
    assert!(plain_output.contains("مِنْ رَبِّهِمْ"), "{plain_output}");
    assert!(simple_output.contains("مِّن رَّبِّهِمْ"), "{simple_output}");
    let plain_plan = plain.plan(source, Formatting::default());
    assert_eq!(plain_plan.edition, plain.edition());
    assert_eq!(
        plain_plan.corpus_sha256,
        "e9f4aa7ba1bea8c5245dc75354e28fd448a423d5d8826f50295416634f506b80"
    );
    assert_eq!(
        simple.plan(source, Formatting::default()).corpus_sha256,
        "01b4ee33d96ebc06964b2afab537833999f603feafff25a4378743115656c897"
    );
}

#[test]
fn plans_check_versions_and_hashes() {
    let detector = Detector::default();
    let source = "قال تعالى قل هو الله أحد";
    let plan = detector.plan(source, Formatting::default());
    let output = plan.apply(source).unwrap();
    assert_eq!(plan.apply(&output), Err(Error::InputHash));
    assert_eq!(plan.revert(source), Err(Error::OutputHash));
    let mut old = plan.clone();
    old.version = "1.0".into();
    assert_eq!(old.apply(source), Err(Error::PlanVersion("1.0".into())));
    let unchanged = detector.plan("قُلْ هُوَ اللَّهُ أَحَدٌ", Formatting::default());
    assert_eq!(unchanged.summary.unchanged, 1);
    assert_eq!(unchanged.edits, []);
}
