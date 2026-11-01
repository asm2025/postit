//! Bounce classification: each SMTP refusal is judged with the stage that gave it, because
//! only `RCPT` speaks about the address.

use postit_agent_email::{Classified, SendRefusal, SmtpStage, classify, parse_enhanced};

fn refusal(stage: SmtpStage, code: u16, enhanced: Option<(u8, u16, u16)>) -> SendRefusal {
    SendRefusal {
        stage,
        code,
        enhanced,
    }
}

#[test]
fn rcpt_address_refusals_bounce() {
    for (code, enhanced) in [
        (550, Some((5, 1, 1))),
        (551, None),
        (553, None),
        (550, None),
        (550, Some((5, 2, 1))),
        (554, Some((5, 1, 1))),
        (552, Some((5, 2, 1))),
        (553, Some((5, 1, 3))),
    ] {
        assert_eq!(
            classify(&refusal(SmtpStage::Rcpt, code, enhanced)),
            Classified::Bounce,
            "{code} {enhanced:?}"
        );
    }
}

#[test]
fn rcpt_policy_and_sender_refusals_are_rejected() {
    for (code, enhanced) in [
        (550, Some((5, 7, 1))),
        (554, None),
        (554, Some((5, 7, 1))),
        (553, Some((5, 7, 26))),
        (550, Some((5, 1, 7))),
        (553, Some((5, 1, 8))),
        (552, None),
    ] {
        assert_eq!(
            classify(&refusal(SmtpStage::Rcpt, code, enhanced)),
            Classified::Rejected,
            "{code} {enhanced:?}"
        );
    }
}

#[test]
fn temporary_refusals_are_soft_at_every_stage() {
    for stage in [SmtpStage::Mail, SmtpStage::Rcpt, SmtpStage::Data] {
        for (code, enhanced) in [(450, Some((4, 2, 0))), (421, None), (451, Some((4, 7, 1)))] {
            assert_eq!(
                classify(&refusal(stage, code, enhanced)),
                Classified::Soft,
                "{stage:?} {code}"
            );
        }
    }
}

#[test]
fn any_permanent_mail_refusal_is_the_sender() {
    for (code, enhanced) in [
        (550, Some((5, 1, 1))),
        (553, Some((5, 1, 8))),
        (550, Some((5, 7, 1))),
        (554, None),
        (551, None),
    ] {
        assert_eq!(
            classify(&refusal(SmtpStage::Mail, code, enhanced)),
            Classified::Sender,
            "{code} {enhanced:?}"
        );
    }
}

#[test]
fn any_permanent_data_refusal_is_rejected() {
    for (code, enhanced) in [
        (550, Some((5, 1, 1))),
        (554, Some((5, 7, 1))),
        (552, None),
        (550, None),
    ] {
        assert_eq!(
            classify(&refusal(SmtpStage::Data, code, enhanced)),
            Classified::Rejected,
            "{code} {enhanced:?}"
        );
    }
}

#[test]
fn enhanced_codes_are_parsed_from_the_reply_text() {
    assert_eq!(
        parse_enhanced("5.1.1 <bob@example.com>: user unknown"),
        Some((5, 1, 1))
    );
    assert_eq!(parse_enhanced("4.7.26 try later"), Some((4, 7, 26)));
    assert_eq!(parse_enhanced("  5.2.1 mailbox disabled"), Some((5, 2, 1)));
    assert_eq!(parse_enhanced("5.7.1"), Some((5, 7, 1)));
    assert_eq!(parse_enhanced("user unknown 5.1.1"), None);
    assert_eq!(parse_enhanced("mailbox unavailable"), None);
    assert_eq!(parse_enhanced("3.1.1 not a failure class"), None);
    assert_eq!(parse_enhanced("5.1 too short"), None);
    assert_eq!(parse_enhanced("5.1.1000 too long"), None);
    assert_eq!(parse_enhanced("5.x.1 not digits"), None);
    assert_eq!(parse_enhanced(""), None);
}

#[test]
fn reasons_are_fixed_codes() {
    assert_eq!(
        refusal(SmtpStage::Rcpt, 550, Some((5, 1, 1))).reason(),
        "smtp_5_1_1"
    );
    assert_eq!(refusal(SmtpStage::Rcpt, 550, None).reason(), "smtp_550");
    assert_eq!(
        refusal(SmtpStage::Data, 451, Some((4, 7, 26))).reason(),
        "smtp_4_7_26"
    );
    assert_eq!(refusal(SmtpStage::Mail, 421, None).reason(), "smtp_421");
}

#[test]
fn sender_policy_refusals_are_recognized() {
    assert!(refusal(SmtpStage::Rcpt, 550, Some((5, 7, 1))).is_sender_policy());
    assert!(refusal(SmtpStage::Data, 550, Some((5, 7, 26))).is_sender_policy());
    assert!(!refusal(SmtpStage::Rcpt, 550, Some((5, 1, 1))).is_sender_policy());
    assert!(!refusal(SmtpStage::Rcpt, 550, None).is_sender_policy());
    assert!(!refusal(SmtpStage::Rcpt, 451, Some((4, 7, 1))).is_sender_policy());
}
