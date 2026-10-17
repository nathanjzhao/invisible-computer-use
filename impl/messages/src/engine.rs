//! Serial per-session dispatch. Candidate content stays private until the host releases it.
use crate::{
    Error, Result,
    activity::Breakdown,
    consent::{Context, Destination, Lease, Operation, Scope, Store},
    contract::{self, Contacts, Invocation},
    cursor::Cursors,
    database::{Chat, MessagePage},
    worker::{AttachmentLocator, Query, Worker},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub enum ReadDecision {
    Accept {
        chat_guids: Vec<String>,
        scope: Scope,
    },
    Decline,
    Cancel,
}
pub enum SendDecision {
    Accept { scope: Scope, text: String },
    Decline,
    Cancel,
}
#[derive(Debug)]
pub enum Output {
    Json(Value),
    Image { mime_type: String, data: String },
}
/// Implemented by the trusted UI/MCP client bridge. Tool arguments cannot implement this trait.
pub trait Host {
    fn review_read(&mut self, chats: &[Chat], cancel: &AtomicBool) -> Result<ReadDecision>;
    fn review_send(
        &mut self,
        _preview: &crate::send::Preview,
        _cancel: &AtomicBool,
    ) -> Result<SendDecision> {
        Err(Error::new(
            "messagesApprovalUnavailable",
            "The Messages host cannot show an editable send approval",
        ))
    }
}
pub struct NoHost;
impl Host for NoHost {
    fn review_read(&mut self, _: &[Chat], _: &AtomicBool) -> Result<ReadDecision> {
        Err(Error::new(
            "messagesApprovalUnavailable",
            "The Messages host cannot show a read approval. No message contents were returned.",
        ))
    }
}

pub struct Engine {
    worker: Worker,
    approvals: Store,
    context: Context,
    cursors: Cursors,
    contacts: Box<dyn Contacts + Send>,
    time_zone: String,
    turn_active: bool,
    attachments: HashMap<String, AttachmentLocator>,
    sender: Option<Box<dyn crate::send::Transport>>,
}
impl Engine {
    pub fn new(
        worker: Worker,
        approvals: Store,
        contacts: Box<dyn Contacts + Send>,
        time_zone: String,
    ) -> Result<Self> {
        // Validate the host's calendar zone even before a count request.
        crate::dates::buckets(0, 0, crate::dates::Interval::Total, &time_zone)?;
        Ok(Self {
            worker,
            approvals,
            context: Context::new_session()?,
            cursors: Cursors::default(),
            contacts,
            time_zone,
            turn_active: true,
            attachments: HashMap::new(),
            sender: None,
        })
    }
    pub fn with_sender(mut self, sender: Box<dyn crate::send::Transport>) -> Self {
        self.sender = Some(sender);
        self
    }
    pub fn next_turn(&mut self) -> Result<()> {
        self.approvals.end_turn(&self.context);
        self.context = self.context.next_turn()?;
        self.turn_active = true;
        Ok(())
    }
    pub fn end_turn(&mut self) {
        self.approvals.end_turn(&self.context);
        self.turn_active = false;
    }
    pub fn call(
        &mut self,
        name: &str,
        args: Value,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<Output> {
        let result = self.call_inner(name, args, host, cancel.clone());
        self.approvals.discard_pending(&self.context);
        if cancel.load(Ordering::SeqCst) && name != "send_message" {
            if matches!(name, "read_messages" | "search_messages" | "read_image") {
                self.approvals.cancel(&self.context, Operation::Read);
            }
            return Err(canceled());
        }
        result
    }
    fn call_inner(
        &mut self,
        name: &str,
        args: Value,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<Output> {
        check_cancel(&cancel)?;
        self.contacts.begin_request(cancel.clone());
        if !self.turn_active {
            return Err(Error::new(
                "messagesTurnEnded",
                "The current turn ended. Messages will be available after the host starts the next turn.",
            ));
        }
        let invocation = contract::parse(name, args, &self.time_zone, self.contacts.as_ref())?;
        let Invocation::Query {
            mut query,
            cursor,
            key,
        } = invocation
        else {
            return match invocation {
                Invocation::Image { id } => self.read_image(&id, host, cancel),
                Invocation::Send { arguments } => self.send_message(arguments, host, cancel),
                _ => unreachable!(),
            };
        };
        let is_content = matches!(*query, Query::Read { .. });
        if is_content {
            self.approvals
                .check_context(&self.context, Operation::Read)?;
        }
        let state = cursor
            .as_deref()
            .map(|token| self.cursors.load(token, key))
            .transpose()?;
        let mut breakdown = Breakdown::Overall;
        match query.as_mut() {
            Query::Read { page, .. } => *page = state,
            Query::Count { query, page, .. } => {
                *page = state;
                breakdown = query.breakdown;
            }
            _ => {}
        }
        let mut response = self.worker.query(*query, cancel.clone())?;
        check_cancel(&cancel)?;
        let mut filtered = false;
        if is_content {
            let mut page: MessagePage =
                serde_json::from_value(response.data).map_err(|_| worker_error())?;
            let requested: BTreeSet<String> =
                page.messages.iter().map(|m| m.chat_guid.clone()).collect();
            let chats: BTreeMap<String, Chat> = response
                .chats
                .into_iter()
                .map(|chat| (chat.chat_guid.clone(), chat))
                .collect();
            if requested.iter().any(|guid| !chats.contains_key(guid)) {
                return Err(worker_error());
            }
            let mut leases: BTreeMap<String, Lease> = BTreeMap::new();
            let mut pending = Vec::new();
            for guid in &requested {
                let chat = &chats[guid];
                match self
                    .approvals
                    .acquire(&self.context, Operation::Read, &destination(chat))
                {
                    Ok(Some(lease)) => {
                        leases.insert(guid.clone(), lease);
                    }
                    Ok(None) => pending.push(chat.clone()),
                    Err(error) if error.code == "messagesReadDeclined" => {}
                    Err(error) => return Err(error),
                }
            }
            if !pending.is_empty() {
                let decision = match host.review_read(&pending, &cancel) {
                    Err(error) if error.code == "messagesApprovalCanceled" => {
                        self.approvals.cancel(&self.context, Operation::Read);
                        return Err(error);
                    }
                    other => other?,
                };
                check_cancel(&cancel)?;
                let (allowed, scope) = match decision {
                    ReadDecision::Cancel => {
                        self.approvals.cancel(&self.context, Operation::Read);
                        return Err(Error::new(
                            "messagesApprovalCanceled",
                            "Content was not returned because read approval was canceled. Do not ask the user to approve access or retry.",
                        ));
                    }
                    ReadDecision::Decline => (BTreeSet::new(), Scope::Once),
                    ReadDecision::Accept { chat_guids, scope } => {
                        let allowed: BTreeSet<String> = chat_guids.into_iter().collect();
                        if allowed
                            .iter()
                            .any(|g| !pending.iter().any(|c| &c.chat_guid == g))
                        {
                            return Err(Error::new(
                                "messagesApprovalInvalid",
                                "The host returned approval for a chat outside this request",
                            ));
                        }
                        (allowed, scope)
                    }
                };
                for chat in &pending {
                    if allowed.contains(&chat.chat_guid) {
                        let target = destination(chat);
                        self.approvals
                            .approve(&self.context, Operation::Read, &target, scope)?;
                        let lease = self
                            .approvals
                            .acquire(&self.context, Operation::Read, &target)?
                            .ok_or_else(worker_error)?;
                        leases.insert(chat.chat_guid.clone(), lease);
                    } else {
                        self.approvals.decline(
                            &self.context,
                            Operation::Read,
                            &destination(chat),
                        )?;
                    }
                }
            }
            let allowed: BTreeSet<String> = leases.keys().cloned().collect();
            if !leases.is_empty() {
                // The user may have left the approval open while chat membership changed.
                let current = self.worker.query(
                    Query::Describe {
                        chat_guids: allowed.iter().cloned().collect(),
                    },
                    cancel.clone(),
                )?;
                let current: BTreeMap<_, _> = current
                    .chats
                    .into_iter()
                    .map(|c| (c.chat_guid.clone(), c))
                    .collect();
                for (guid, lease) in leases {
                    let chat = current.get(&guid).ok_or_else(worker_error)?;
                    self.approvals.consume(
                        lease,
                        &self.context,
                        Operation::Read,
                        &destination(chat),
                    )?;
                }
            }
            check_cancel(&cancel)?;
            filtered = page
                .messages
                .iter()
                .any(|m| !allowed.contains(&m.chat_guid));
            page.messages.retain(|m| allowed.contains(&m.chat_guid));
            if name == "read_messages" && filtered {
                return Err(Error::new(
                    "messagesReadDeclined",
                    "Content was not returned because access to this chat was declined. Do not ask again or retry during this turn.",
                ));
            }
            response
                .attachments
                .retain(|a| allowed.contains(&a.chat_guid));
            response.data = serde_json::to_value(page).map_err(|_| worker_error())?;
        }
        // Pagination follows the complete candidate page, including messages filtered by permission.
        if let Some(page) = response.next_page {
            response.data["next_cursor"] = json!(self.cursors.save(key, page)?);
        }
        let mut result = contract::render(name, response.data, self.contacts.as_ref(), breakdown)?;
        if filtered {
            result["permission_filtered"] = json!(true);
            result["permission_filter_message"] =
                json!("Messages from chats without read permission were omitted from this page.");
        }
        check_cancel(&cancel)?;
        if !response.attachments.is_empty() {
            let incoming: BTreeSet<_> =
                response.attachments.iter().map(|a| a.id.as_str()).collect();
            let new = incoming
                .iter()
                .filter(|id| !self.attachments.contains_key(**id))
                .count();
            if self.attachments.len() + new > 1000 {
                // Keep every reference in this response, even when one page exceeds the soft cache cap.
                // The worker's response-size limit bounds the largest single page.
                self.attachments
                    .retain(|id, _| incoming.contains(id.as_str()));
            }
        }
        for locator in response.attachments {
            self.attachments.insert(locator.id.clone(), locator);
        }
        Ok(Output::Json(result))
    }
    fn read_image(
        &mut self,
        id: &str,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<Output> {
        self.approvals
            .check_context(&self.context, Operation::Read)?;
        let locator = self.attachments.get(id).cloned().ok_or_else(|| {
            Error::new(
                "messagesAttachmentNotFound",
                "This attachment reference is unavailable. Read the message again to obtain a current attachment id.",
            )
        })?;
        let mut described = self.worker.query(
            Query::Describe {
                chat_guids: vec![locator.chat_guid.clone()],
            },
            cancel.clone(),
        )?;
        let chat = described.chats.pop().ok_or_else(worker_error)?;
        let target = destination(&chat);
        let lease = match self
            .approvals
            .acquire(&self.context, Operation::Read, &target)?
        {
            Some(lease) => lease,
            None => {
                let decision = host.review_read(std::slice::from_ref(&chat), &cancel);
                check_cancel(&cancel)?;
                let decision = match decision {
                    Err(error) if error.code == "messagesApprovalCanceled" => {
                        self.approvals.cancel(&self.context, Operation::Read);
                        return Err(error);
                    }
                    other => other?,
                };
                match decision {
                    ReadDecision::Accept { chat_guids, scope }
                        if chat_guids.as_slice() == [chat.chat_guid.clone()] =>
                    {
                        self.approvals
                            .approve(&self.context, Operation::Read, &target, scope)?
                    }
                    ReadDecision::Cancel => {
                        self.approvals.cancel(&self.context, Operation::Read);
                        return Err(Error::new(
                            "messagesApprovalCanceled",
                            "Image approval was canceled. Do not ask again or retry during this turn.",
                        ));
                    }
                    ReadDecision::Accept { chat_guids, .. } if !chat_guids.is_empty() => {
                        return Err(Error::new(
                            "messagesApprovalInvalid",
                            "The host approved a different chat",
                        ));
                    }
                    _ => {
                        self.approvals
                            .decline(&self.context, Operation::Read, &target)?;
                        return Err(Error::new(
                            "messagesReadDeclined",
                            "Image access was declined for this chat during this turn.",
                        ));
                    }
                }
                self.approvals
                    .acquire(&self.context, Operation::Read, &target)?
                    .ok_or_else(worker_error)?
            }
        };
        let image = self.worker.query(
            Query::Image {
                chat_guid: locator.chat_guid.clone(),
                message_guid: locator.message_guid,
                attachment_id: locator.id,
            },
            cancel.clone(),
        )?;
        let image: crate::attachment_image::Image =
            serde_json::from_value(image.data).map_err(|_| worker_error())?;
        let mut current = self.worker.query(
            Query::Describe {
                chat_guids: vec![locator.chat_guid],
            },
            cancel.clone(),
        )?;
        let current = current.chats.pop().ok_or_else(worker_error)?;
        self.approvals.consume(
            lease,
            &self.context,
            Operation::Read,
            &destination(&current),
        )?;
        check_cancel(&cancel)?;
        if image.mime_type != "image/png"
            || image.data.len() > crate::attachment_image::MAX_IMAGE.div_ceil(3) * 4
        {
            return Err(worker_error());
        }
        Ok(Output::Image {
            mime_type: image.mime_type,
            data: image.data,
        })
    }
    fn send_message(
        &mut self,
        args: Value,
        host: &mut dyn Host,
        cancel: Arc<AtomicBool>,
    ) -> Result<Output> {
        use crate::send::Prepared;
        if self.sender.is_none() {
            return Err(Error::new(
                "messagesSendUnavailable",
                "No native send transport is connected; no message was sent",
            ));
        }
        self.approvals
            .check_context(&self.context, Operation::Send)?;
        let request =
            serde_json::from_value(args).map_err(|_| crate::invalid("Invalid send request"))?;
        let plan = Prepared::create(
            request,
            &self.worker,
            self.contacts.as_ref(),
            cancel.clone(),
        )?;
        let destination = plan.consent();
        let existing = destination
            .as_ref()
            .map(|d| self.approvals.acquire(&self.context, Operation::Send, d))
            .transpose()?
            .flatten();
        let (approved, lease) = if let Some(lease) = existing {
            let text = plan.preview().text.unwrap_or_default();
            (plan.approve(text)?, Some(lease))
        } else {
            let decision = host.review_send(&plan.preview(), &cancel);
            if cancel.load(Ordering::SeqCst) {
                self.approvals.cancel(&self.context, Operation::Send);
                return Err(Error::new(
                    "messagesSendCanceled",
                    "The send was canceled before approval was accepted",
                ));
            }
            let decision = match decision {
                Err(error) if error.code == "messagesApprovalCanceled" => {
                    self.approvals.cancel(&self.context, Operation::Send);
                    return Err(error);
                }
                other => other?,
            };
            match decision {
                SendDecision::Accept { scope, text } => {
                    if destination.is_none() && scope != Scope::Once {
                        return Err(Error::new(
                            "messagesApprovalInvalid",
                            "A new conversation can only receive approval for this send",
                        ));
                    }
                    let approved = plan.approve(text)?;
                    let lease = if let Some(target) = &destination {
                        self.approvals
                            .approve(&self.context, Operation::Send, target, scope)?;
                        self.approvals
                            .acquire(&self.context, Operation::Send, target)?
                    } else {
                        None
                    };
                    (approved, lease)
                }
                SendDecision::Cancel | SendDecision::Decline => {
                    self.approvals.cancel(&self.context, Operation::Send);
                    return Err(Error::new(
                        "messagesSendCanceled",
                        "Message was not sent because send approval was canceled or declined. Do not ask again or retry during this turn.",
                    ));
                }
            }
        };
        let approvals = &mut self.approvals;
        let context = &self.context;
        let outcome = approved.commit_authorized(
            &self.worker,
            self.contacts.as_ref(),
            self.sender.as_mut().unwrap().as_mut(),
            cancel,
            || {
                if let (Some(lease), Some(target)) = (lease, destination) {
                    approvals.consume(lease, context, Operation::Send, &target)?;
                }
                Ok(())
            },
        );
        match outcome {
            Ok(sent) => Ok(Output::Json(
                json!({"status":"sent","chat_guid":sent.chat_guid,"user_edited":sent.user_edited,"attachment_count":sent.attachment_count}),
            )),
            Err(error) => {
                if matches!(
                    error.code.as_str(),
                    "messagesSendOutcomeUncertain" | "messagesSendCanceled"
                ) {
                    self.approvals.cancel(&self.context, Operation::Send);
                }
                Err(error)
            }
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.approvals.end_session(&self.context);
    }
}
fn destination(chat: &Chat) -> Destination {
    Destination {
        chat_guid: chat.chat_guid.clone(),
        participants: chat.participants.clone(),
        service: chat.service.clone().unwrap_or_else(|| "unknown".into()),
    }
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(canceled())
    } else {
        Ok(())
    }
}
fn canceled() -> Error {
    Error::new(
        "messagesRequestCanceled",
        "Messages request was canceled; no message contents were returned",
    )
}
fn worker_error() -> Error {
    Error::new(
        "messagesWorkerFailed",
        "Messages query worker returned incomplete data",
    )
}
