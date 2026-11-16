// Copyright © Klarälvdalens Datakonsult AB, a KDAB Group company, info@kdab.com, author Robin Cramer <robin.cramer@kdab.com>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

#![cfg(feature = "system-testing")]

use i_slint_backend_testing::proto::{self, aut_response::Msg as Resp, request_to_aut::Msg as Req};
use i_slint_live_preview::protocol::{
    PreviewToDebuggerMessage, PreviewToLsp, PreviewToLspMessage, Result,
};
use i_slint_live_preview::system_testing::answer_request;
use prost::Message;
use slint_interpreter::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct CaptureToDebugger(RefCell<Vec<PreviewToDebuggerMessage>>);

impl PreviewToLsp for CaptureToDebugger {
    fn send(&self, message: &PreviewToLspMessage) -> Result<()> {
        let PreviewToLspMessage::DebuggerMessage(message) = message else {
            unreachable!("System-test responses go to the debugger")
        };
        self.0.borrow_mut().push(message.clone());
        Ok(())
    }
}

/// One test, since the testing backend's event loop can only be set up once per process.
#[test]
fn answers_and_pushes_on_the_event_loop() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    i_slint_backend_testing::systest::init().unwrap();
    let capture = Rc::new(CaptureToDebugger::default());
    let to_editor: Rc<dyn PreviewToLsp> = capture.clone();
    answer_request(7, vec![0xff], Rc::downgrade(&to_editor));
    let subscribe_to_window_roots = proto::RequestToAut {
        msg: Some(Req::RequestSubscribeElementChildren(proto::RequestSubscribeElementChildren {
            element_handle: None,
        })),
    };
    answer_request(8, subscribe_to_window_roots.encode_to_vec(), Rc::downgrade(&to_editor));
    i_slint_backend_selector::with_global_context(|context| {
        context.spawn_local(async { i_slint_core::api::quit_event_loop().unwrap() }).unwrap();
        context.run_event_loop().unwrap();
    })
    .unwrap();
    let messages = capture.0.take();
    let [
        PreviewToDebuggerMessage::SystemTestResponse { id: 7, response: Ok(response) },
        PreviewToDebuggerMessage::SystemTestResponse { id: 8, response: Ok(subscribed) },
    ] = messages.as_slice()
    else {
        panic!("expected two responses, got {messages:?}");
    };
    assert!(!response.is_empty());
    let Some(Resp::SubscribeElementChildrenResponse(subscribed)) =
        proto::AutResponse::decode(&subscribed[..]).unwrap().msg
    else {
        panic!("expected a children subscription");
    };

    let compiled = tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(
        slint_interpreter::Compiler::default().build_from_source(
            "export component App inherits Window {}".into(),
            "/app.slint".into(),
        ),
    );
    let app = compiled.component("App").unwrap().create().unwrap();
    app.show().unwrap();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    let messages = capture.0.take();
    let [PreviewToDebuggerMessage::SubscriptionNotification { notification }] = messages.as_slice()
    else {
        panic!("expected a notification, got {messages:?}");
    };
    let notification = proto::SubscriptionNotification::decode(&notification[..]).unwrap();
    assert_eq!(notification.subscription_id, subscribed.subscription_id);
    assert!(matches!(
        notification.msg,
        Some(proto::subscription_notification::Msg::ChildrenChanged(_))
    ));
}
