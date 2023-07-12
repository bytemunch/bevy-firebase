mod googleapis;

use std::fs::read_to_string;
use std::{collections::HashMap, path::PathBuf};

pub use crate::googleapis::google::firestore::v1::*;

use bevy::prelude::*;

use bevy_firebase_auth::{AuthState, ProjectId, TokenData};
use bevy_tokio_tasks::TokioTasksRuntime;

use futures_lite::{stream, StreamExt};

use googleapis::google::firestore::v1::firestore_client::FirestoreClient;
use googleapis::google::firestore::v1::listen_request::TargetChange;
use googleapis::google::firestore::v1::run_query_request::QueryType;
use googleapis::google::firestore::v1::structured_query::{
    CollectionSelector, FieldReference, Order,
};
use googleapis::google::firestore::v1::target::{DocumentsTarget, TargetType};
use tonic::{
    codegen::InterceptedService,
    metadata::{Ascii, MetadataValue},
    service::Interceptor,
    transport::{Certificate, Channel, ClientTlsConfig},
    Request, Response,
};

pub use googleapis::google::firestore::v1::structured_query::Direction as QueryDirection;
pub use tonic::Status;

// FIRESTORE
#[derive(Resource, Clone)]
pub struct BevyFirestoreClient(
    pub FirestoreClient<InterceptedService<Channel, FirebaseInterceptor>>,
);

#[derive(Resource, Clone)]
struct EmulatorUrl(String);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum FirestoreState {
    #[default]
    Start,
    Init,
    CreateClient,
    Ready,
}

#[derive(Clone)]
pub struct FirebaseInterceptor {
    bearer_token: MetadataValue<Ascii>,
    db: MetadataValue<Ascii>,
}

impl Interceptor for FirebaseInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.bearer_token.clone());

        request
            .metadata_mut()
            .insert("google-cloud-resource-prefix", self.db.clone());
        Ok(request)
    }
}

#[derive(Default)]
pub struct FirestorePlugin {
    pub emulator_url: Option<String>,
}

impl Plugin for FirestorePlugin {
    fn build(&self, app: &mut App) {
        // TODO refresh client token when app token is refreshed
        if self.emulator_url.is_some() {
            app.insert_resource(EmulatorUrl(self.emulator_url.clone().unwrap()));
        }

        app.add_state::<FirestoreState>()
            .add_system(logged_in.in_schedule(OnEnter(AuthState::LoggedIn)))
            .add_system(init.in_schedule(OnEnter(FirestoreState::Init)))
            .add_system(create_client.in_schedule(OnEnter(FirestoreState::CreateClient)))
            // LISTENER
            .add_event::<CreateListenerEvent>()
            .add_event::<ListenerResponseEvent>()
            .add_system(
                create_listener_event_handler::<CreateListenerEvent, ListenerResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            )
            // QUERY
            .add_event::<QueryResponseEvent>()
            .add_event::<RunQueryEvent>()
            .add_system(
                run_query_event_handler::<RunQueryEvent, QueryResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            )
            // CREATE
            // Events
            .add_event::<CreateDocumentEvent>()
            .add_event::<CreateDocumentResponseEvent>()
            // Event Readers
            .add_system(
                create_document_event_handler::<CreateDocumentEvent, CreateDocumentResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            )
            // UPDATE
            // Events
            .add_event::<UpdateDocumentEvent>()
            .add_event::<UpdateDocumentResponseEvent>()
            // Event Readers
            .add_system(
                update_document_event_handler::<UpdateDocumentEvent, UpdateDocumentResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            )
            // READ
            // Events
            .add_event::<ReadDocumentEvent>()
            .add_event::<ReadDocumentResponseEvent>()
            // Event Readers
            .add_system(
                read_document_event_handler::<ReadDocumentEvent, ReadDocumentResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            )
            // DELETE
            // Events
            .add_event::<DeleteDocumentEvent>()
            .add_event::<DeleteDocumentResponseEvent>()
            // Event Readers
            .add_system(
                delete_document_event_handler::<DeleteDocumentEvent, DeleteDocumentResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            );
    }
}

fn logged_in(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::Init);
}

fn init(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::CreateClient);
}

// CLIENT

fn create_client(
    runtime: ResMut<TokioTasksRuntime>,
    user_info: Res<TokenData>,
    emulator: Option<Res<EmulatorUrl>>,
    project_id: Res<ProjectId>,
) {
    let id_token = user_info.id_token.clone();
    let project_id = project_id.0.clone();

    let emulator_url = match emulator {
        Some(e) => Some(e.0.clone()),
        None => None,
    };

    // CREATE BG TASK TO INSERT CLIENT AS RESOURCE
    runtime.spawn_background_task(|mut ctx| async move {
        let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
        let certs = read_to_string(data_dir.join("gcp/gtsr1.pem")).unwrap();

        let channel = if emulator_url.is_none() {
            let tls_config = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(certs))
                .domain_name("firestore.googleapis.com");

            Channel::from_static("https://firestore.googleapis.com")
                .tls_config(tls_config)
                .unwrap()
                .connect()
                .await
                .unwrap()
        } else {
            Channel::from_shared(emulator_url.unwrap())
                .unwrap()
                .connect()
                .await
                .unwrap()
        };

        let service = FirestoreClient::with_interceptor(
            channel,
            FirebaseInterceptor {
                bearer_token: format!("Bearer {}", id_token).parse().unwrap(),
                db: format!("projects/{}/databases/(default)", project_id.clone())
                    .parse()
                    .unwrap(),
            },
        );

        ctx.run_on_main_thread(move |ctx| {
            ctx.world.insert_resource(BevyFirestoreClient(service));

            ctx.world
                .insert_resource(NextState(Some(FirestoreState::Ready)));
        })
        .await;
    });
}

// LISTENER

pub trait ListenerResponseEventBuilder {
    fn new(msg: ListenResponse) -> Self;
    fn msg(&self) -> ListenResponse {
        ListenResponse {
            ..Default::default()
        }
    }
}

pub struct ListenerResponseEvent {
    pub msg: ListenResponse,
}

impl ListenerResponseEventBuilder for ListenerResponseEvent {
    fn new(msg: ListenResponse) -> Self {
        ListenerResponseEvent { msg }
    }
    fn msg(&self) -> ListenResponse {
        self.msg.clone()
    }
}

pub fn add_listener<T>(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut Client,
    project_id: String,
    target: String,
) where
    T: ListenerResponseEventBuilder + std::marker::Send + std::marker::Sync + 'static,
{
    let mut client = client.clone();

    runtime.spawn_background_task(|mut ctx| async move {
        let db = format!("projects/{project_id}/databases/(default)");
        let req = ListenRequest {
            database: db.clone(),
            labels: HashMap::new(),
            target_change: Some(TargetChange::AddTarget(Target {
                target_id: 0x52757374, // rust in hex, for... reasons?
                once: false,
                resume_type: None,
                target_type: Some(TargetType::Documents(DocumentsTarget {
                    documents: vec![db + "/documents/" + &*target],
                })),
                ..Default::default()
            })),
        };

        let req = Request::new(stream::iter(vec![req]).chain(stream::pending()));

        // TODO handle errors
        let res = client.listen(req).await.unwrap();

        let mut res = res.into_inner();

        while let Some(msg) = res.next().await {
            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(T::new(msg.unwrap()));
            })
            .await;
        }
    });
}

pub trait CreateListenerEventBuilder {
    fn target(&self) -> String;
}

/// Responding to listener events:
///
/// ```
/// fn listener_response_event_handler<T>(mut er: EventReader<T>)
/// where
///     T: Send + Sync + 'static + ListenerResponseEventBuilder,
/// {
///     for ev in er.iter() {
///         match ev.msg().response_type.as_ref().unwrap() {
///             ResponseType::TargetChange(response) => {
///                 let change_type = response.target_change_type;
///                 match change_type {
///                     0 => {
///                         // no change
///                     }
///                     1 => {
///                         // target added
///                     }
///                     2 => {
///                         // target removed
///                     }
///                     3 => {
///                         // target current (research needed lol)
///                     }
///                     4 => {
///                         // reset (also no idea)
///                     }
///                     _ => {
///                         // unknown response
///                     }
///                 }
///             }
///             ResponseType::DocumentChange(response) => {
///                 println!("Document Changed: {:?}", response.document.clone().unwrap());
///             }
///             ResponseType::DocumentDelete(response) => {
///                 println!("Document Deleted: {:?}", response.document.clone());
///             }
///             ResponseType::DocumentRemove(response) => {
///                 println!("Document Removed: {:?}", response.document.clone());
///             }
///             ResponseType::Filter(response) => {
///                 println!("Filter: {:?}", response);
///             }
///         }
///     }
/// }
/// ```
pub struct CreateListenerEvent {
    pub target: String,
}

impl CreateListenerEventBuilder for CreateListenerEvent {
    fn target(&self) -> String {
        self.target.clone()
    }
}

pub fn create_listener_event_handler<T, R>(
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
    mut client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
) where
    T: CreateListenerEventBuilder + Send + Sync + 'static,
    R: ListenerResponseEventBuilder + Send + Sync + 'static,
{
    for e in er.iter() {
        add_listener::<R>(
            &runtime,
            &mut client.0,
            project_id.0.clone(),
            e.target().clone(),
        )
    }
}

// QUERY

type QueryResponse = Result<Vec<RunQueryResponse>, Status>;
pub trait QueryResponseEventBuilder {
    fn new(msg: QueryResponse) -> Self;
    fn msg(&self) -> QueryResponse;
}

pub struct QueryResponseEvent {
    pub msg: QueryResponse,
}

impl QueryResponseEventBuilder for QueryResponseEvent {
    fn new(msg: QueryResponse) -> Self {
        QueryResponseEvent { msg }
    }
    fn msg(&self) -> QueryResponse {
        self.msg.clone()
    }
}

pub trait RunQueryEventBuilder {
    fn parent(&self) -> String;
    fn collection_id(&self) -> String;
    fn limit(&self) -> Option<i32>;
    fn order_by(&self) -> (String, QueryDirection);
}

pub struct RunQueryEvent {
    pub parent: String,
    pub collection_id: String,
    pub limit: Option<i32>,
    pub order_by: (String, QueryDirection),
}

impl RunQueryEventBuilder for RunQueryEvent {
    fn collection_id(&self) -> String {
        self.collection_id.clone()
    }
    fn limit(&self) -> Option<i32> {
        self.limit
    }
    fn order_by(&self) -> (String, QueryDirection) {
        self.order_by.clone()
    }
    fn parent(&self) -> String {
        self.parent.clone()
    }
}

pub fn run_query_event_handler<T, R>(
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
    mut client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
) where
    T: RunQueryEventBuilder + Send + Sync + 'static,
    R: QueryResponseEventBuilder + Send + Sync + 'static,
{
    for e in er.iter() {
        run_query::<R>(
            &runtime,
            &mut client.0,
            project_id.0.clone(),
            e.parent(),
            e.collection_id(),
            e.limit(),
            e.order_by(),
        )
    }
}

// pub fn query_response_event_handler(mut er: EventReader<QueryResponseEvent>) {
//     for e in er.iter() {
//         println!("QUERY: {:?}", e.msg)
//     }
// }

pub fn run_query<T>(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut Client,
    project_id: String,
    parent: String,
    collection_id: String,
    limit: Option<i32>,
    order_by: (String, QueryDirection),
) where
    T: QueryResponseEventBuilder + Send + Sync + 'static,
{
    let parent = if !parent.is_empty() {
        format!("/{parent}")
    } else {
        "".into()
    };

    let mut client = client.clone();

    let order_field = order_by.0.clone();
    let order_direction = order_by.1;

    runtime.spawn_background_task(move |mut ctx| async move {
        let req = RunQueryRequest {
            parent: format!("projects/{project_id}/databases/(default)/documents{parent}"),
            query_type: Some(QueryType::StructuredQuery(StructuredQuery {
                from: vec![CollectionSelector {
                    collection_id,
                    all_descendants: false,
                }],
                limit,
                order_by: vec![Order {
                    field: Some(FieldReference {
                        field_path: order_field,
                    }),
                    direction: order_direction as i32,
                }],
                ..Default::default()
            })),
            ..Default::default()
        };

        let mut res = client.run_query(req).await.unwrap().into_inner();

        let mut responses = Vec::new();

        let mut response_result = Ok(Vec::new());

        while let Some(msg) = res.next().await {
            match msg {
                Ok(msg) => {
                    responses.push(msg.clone());

                    if let Some(_continuation_selector) = msg.continuation_selector {
                        // Break when at end of results
                        break;
                    }
                }
                Err(err) => {
                    responses.clear();
                    response_result = Err(err);
                    // break on error
                    break;
                }
            }
        }

        response_result = match response_result {
            Ok(_) => Ok(responses),
            Err(err) => Err(err),
        };

        ctx.run_on_main_thread(move |ctx| {
            ctx.world.send_event(T::new(response_result));
        })
        .await;
    });
}

// CRUD

pub async fn async_create_document(
    client: &mut Client,
    project_id: &String,
    document_id: &String,
    collection_id: &String,
    fields: HashMap<String, Value>,
) -> Result<Response<Document>, Status> {
    client
        .create_document(CreateDocumentRequest {
            parent: format!("projects/{project_id}/databases/(default)/documents"),
            collection_id: collection_id.into(),
            document_id: document_id.into(),
            document: Some(Document {
                fields,
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
}

pub async fn async_update_document(
    client: &mut Client,
    project_id: &String,
    document_path: &String,
    fields: HashMap<String, Value>,
) -> Result<Response<Document>, Status> {
    let field_paths = Vec::from_iter(fields.clone().keys().cloned());

    client
        .update_document(UpdateDocumentRequest {
            document: Some(Document {
                name: format!(
                    "projects/{project_id}/databases/(default)/documents/{document_path}"
                ),
                fields,
                ..Default::default()
            }),
            update_mask: Some(DocumentMask { field_paths }),
            ..Default::default()
        })
        .await
}

pub async fn async_read_document(
    client: &mut Client,
    project_id: &String,
    document_path: &String,
) -> Result<Response<Document>, Status> {
    client
        .get_document(GetDocumentRequest {
            name: format!("projects/{project_id}/databases/(default)/documents/{document_path}"),
            ..Default::default()
        })
        .await
}

pub async fn async_delete_document(
    client: &mut Client,
    project_id: &String,
    document_path: &String,
) -> Result<Response<()>, Status> {
    client
        .delete_document(DeleteDocumentRequest {
            name: format!("projects/{project_id}/databases/(default)/documents/{document_path}"),
            ..Default::default()
        })
        .await
}

pub type DocumentResult = Result<Document, Status>;
pub type Client = FirestoreClient<InterceptedService<Channel, FirebaseInterceptor>>;

// CREATE

// TODO make all of these event driven
pub trait CreateDocumentEventBuilder {
    fn new(options: Self) -> Self;
    fn document_id(&self) -> String {
        "".into()
    }
    fn collection_id(&self) -> String {
        "".into()
    }
    fn document_data(&self) -> HashMap<String, Value> {
        let h: HashMap<String, Value> = HashMap::new();
        h
    }
}

#[derive(Clone)]
pub struct CreateDocumentEvent {
    pub document_id: String,
    pub collection_id: String,
    pub document_data: HashMap<String, Value>,
}

impl CreateDocumentEventBuilder for CreateDocumentEvent {
    fn new(options: CreateDocumentEvent) -> Self {
        options
    }
    fn collection_id(&self) -> String {
        self.collection_id.clone()
    }
    fn document_data(&self) -> HashMap<String, Value> {
        self.document_data.clone()
    }
    fn document_id(&self) -> String {
        self.document_id.clone()
    }
}

pub trait CreateDocumentResponseEventBuilder {
    fn new(result: DocumentResult) -> Self;
}

#[derive(Clone)]
pub struct CreateDocumentResponseEvent {
    pub result: DocumentResult,
}

impl CreateDocumentResponseEventBuilder for CreateDocumentResponseEvent {
    fn new(result: DocumentResult) -> Self {
        CreateDocumentResponseEvent { result }
    }
}

pub fn create_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: CreateDocumentEventBuilder + Send + Sync + 'static + Clone,
    R: CreateDocumentResponseEventBuilder + Send + Sync + 'static + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let collection_id = e.collection_id();
        let document_id = e.document_id();
        let fields = e.document_data();

        runtime.spawn_background_task(|mut ctx| async move {
            let response = async_create_document(
                &mut client,
                &project_id,
                &document_id,
                &collection_id,
                fields,
            )
            .await;

            let result = match response {
                Ok(result) => Ok(result.into_inner()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result));
            })
            .await;
        });
    }
}

// TODO response event handlers by user; example in docs
// TODO one example for each of CRUD

// fn create_document_response_event_handler(mut er: EventReader<CreateDocumentResponseEvent>) {
//     for e in er.iter() {
//         match e.result.clone() {
//             Ok(result) => {
//                 println!("Document created: {:?}", result)
//             }
//             Err(status) => {
//                 println!("ERROR: Document create failed: {}", status)
//             }
//         }
//     }
// }

// UPDATE
pub trait UpdateDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_path(&self) -> String {
        "".into()
    }
    fn document_data(&self) -> HashMap<String, Value> {
        let h: HashMap<String, Value> = HashMap::new();
        h
    }
}

#[derive(Clone)]
pub struct UpdateDocumentEvent {
    pub document_path: String,
    pub document_data: HashMap<String, Value>,
}

impl UpdateDocumentEventBuilder for UpdateDocumentEvent {
    fn new(event: UpdateDocumentEvent) -> Self {
        event
    }
    fn document_data(&self) -> HashMap<String, Value> {
        self.document_data.clone()
    }
    fn document_path(&self) -> String {
        self.document_path.clone()
    }
}

pub trait UpdateDocumentResponseEventBuilder {
    fn new(result: DocumentResult) -> Self;
}

#[derive(Clone)]
pub struct UpdateDocumentResponseEvent {
    pub result: DocumentResult,
}

impl UpdateDocumentResponseEventBuilder for UpdateDocumentResponseEvent {
    fn new(result: DocumentResult) -> Self {
        UpdateDocumentResponseEvent { result }
    }
}

fn update_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: UpdateDocumentEventBuilder + Send + Sync + 'static + Clone,
    R: UpdateDocumentResponseEventBuilder + Send + Sync + 'static + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let document_path = e.document_path();
        let fields = e.document_data();

        runtime.spawn_background_task(|mut ctx| async move {
            let response =
                async_update_document(&mut client, &project_id, &document_path, fields).await;

            // TODO DRY

            let result = match response {
                Ok(result) => Ok(result.into_inner()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result));
            })
            .await;
        });
    }
}

// READ

pub trait ReadDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_path(&self) -> String {
        "".into()
    }
}

#[derive(Clone)]
pub struct ReadDocumentEvent {
    pub document_path: String,
}

impl ReadDocumentEventBuilder for ReadDocumentEvent {
    fn new(event: ReadDocumentEvent) -> Self {
        event
    }
    fn document_path(&self) -> String {
        self.document_path.clone()
    }
}

pub trait ReadDocumentResponseEventBuilder {
    fn new(result: DocumentResult) -> Self;
}

#[derive(Clone)]
pub struct ReadDocumentResponseEvent {
    pub result: DocumentResult,
}

impl ReadDocumentResponseEventBuilder for ReadDocumentResponseEvent {
    fn new(result: DocumentResult) -> Self {
        ReadDocumentResponseEvent { result }
    }
}

fn read_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: ReadDocumentEventBuilder + Send + Sync + 'static + Clone,
    R: ReadDocumentResponseEventBuilder + Send + Sync + 'static + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let document_path = e.document_path();

        runtime.spawn_background_task(|mut ctx| async move {
            let response = async_read_document(&mut client, &project_id, &document_path).await;

            // TODO DRY

            let result = match response {
                Ok(result) => Ok(result.into_inner()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result));
            })
            .await;
        });
    }
}

// DELETE

pub trait DeleteDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_path(&self) -> String {
        "".into()
    }
}

#[derive(Clone)]
pub struct DeleteDocumentEvent {
    pub document_path: String,
}

impl DeleteDocumentEventBuilder for DeleteDocumentEvent {
    fn new(event: DeleteDocumentEvent) -> Self {
        event
    }
    fn document_path(&self) -> String {
        self.document_path.clone()
    }
}

pub trait DeleteDocumentResponseEventBuilder {
    fn new(result: Result<(), Status>) -> Self;
}

#[derive(Clone)]
pub struct DeleteDocumentResponseEvent {
    pub result: Result<(), Status>,
}

impl DeleteDocumentResponseEventBuilder for DeleteDocumentResponseEvent {
    fn new(result: Result<(), Status>) -> Self {
        DeleteDocumentResponseEvent { result }
    }
}

fn delete_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: DeleteDocumentEventBuilder + Send + Sync + 'static + Clone,
    R: DeleteDocumentResponseEventBuilder + Send + Sync + 'static + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let document_path = e.document_path();

        runtime.spawn_background_task(|mut ctx| async move {
            let response = async_delete_document(&mut client, &project_id, &document_path).await;

            // TODO DRY

            let result = match response {
                Ok(_) => Ok(()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result));
            })
            .await;
        });
    }
}
