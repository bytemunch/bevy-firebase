mod googleapis;

use std::fs::read_to_string;
use std::{collections::HashMap, path::PathBuf};

pub use crate::googleapis::google::firestore::v1::*;
pub use googleapis::google::firestore::v1::structured_query::Direction as QueryDirection;
pub use tonic::Status;

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

// FIRESTORE

/// Bevy `Resource` that holds the Firestore RPC client
///
/// This is inserted during the `FirestorePlugin` instantiation, and is meant
/// to provide access to the RPC bindings from anywhere in the app.
#[derive(Resource, Clone)]
pub struct BevyFirestoreClient(
    pub FirestoreClient<InterceptedService<Channel, FirebaseInterceptor>>,
);

/// Adds authorization headers to RPC requests
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

/// Firestore connection status. Use Firestore only when this is `FirestoreState::Ready`
#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum FirestoreState {
    #[default]
    Start,
    Init,
    CreateClient,
    Ready,
}

/// Bevy plugin for Firestore systems. Expects access to resources added
/// by bevy-firebase-auth: `TokenData`, `AuthState` and `ProjectId`
///
/// # Examples
///
/// With emulated Firestore:
/// ```
/// App::new()
///     .add_plugins(FirestorePlugin {
///         emulator_url: Some("http://127.0.0.1:8080".into()),
///     });
/// ```
///
/// Live Firestore:
/// ```
/// App::new()
///     .add_plugins(FirestorePlugin::default());
/// ```
#[derive(Default)]
pub struct FirestorePlugin {
    pub emulator_url: Option<String>,
}

#[derive(Resource, Clone)]
struct EmulatorUrl(String);

impl Plugin for FirestorePlugin {
    fn build(&self, app: &mut App) {
        if self.emulator_url.is_some() {
            app.insert_resource(EmulatorUrl(self.emulator_url.clone().unwrap()));
        }

        app.add_state::<FirestoreState>()
            .add_systems(OnEnter(AuthState::LoggedIn), logged_in)
            .add_systems(OnEnter(FirestoreState::Init), init)
            .add_systems(OnEnter(FirestoreState::CreateClient), create_client)
            // LISTENER
            .add_event::<CreateListenerEvent>()
            .add_event::<ListenerResponseEvent>()
            .add_systems(
                Update,
                create_listener_event_handler::<CreateListenerEvent, ListenerResponseEvent>
                    .run_if(in_state(FirestoreState::Ready)),
            )
            // QUERY
            .add_event::<QueryResponseEvent>()
            .add_event::<RunQueryEvent>()
            .add_systems(
                Update,
                run_query_event_handler::<RunQueryEvent, QueryResponseEvent>
                    .run_if(in_state(FirestoreState::Ready)),
            )
            // CREATE
            .add_event::<CreateDocumentEvent>()
            .add_event::<CreateDocumentResponseEvent>()
            .add_systems(
                Update,
                create_document_event_handler::<CreateDocumentEvent, CreateDocumentResponseEvent>
                    .run_if(in_state(FirestoreState::Ready)),
            )
            // UPDATE
            .add_event::<UpdateDocumentEvent>()
            .add_event::<UpdateDocumentResponseEvent>()
            .add_systems(
                Update,
                update_document_event_handler::<UpdateDocumentEvent, UpdateDocumentResponseEvent>
                    .run_if(in_state(FirestoreState::Ready)),
            )
            // READ
            .add_event::<ReadDocumentEvent>()
            .add_event::<ReadDocumentResponseEvent>()
            .add_systems(
                Update,
                read_document_event_handler::<ReadDocumentEvent, ReadDocumentResponseEvent>
                    .run_if(in_state(FirestoreState::Ready)),
            )
            // DELETE
            .add_event::<DeleteDocumentEvent>()
            .add_event::<DeleteDocumentResponseEvent>()
            .add_systems(
                Update,
                delete_document_event_handler::<DeleteDocumentEvent, DeleteDocumentResponseEvent>
                    .run_if(in_state(FirestoreState::Ready)),
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

/// Implement this to create custom listener response events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl ListenerResponseEventBuilder for ListenerResponseEvent {
///    fn new(msg: ListenResponse) -> Self {
///        ListenerResponseEvent { msg }
///    }
///    fn msg(&self) -> ListenResponse {
///        self.msg.clone()
///    }
/// }
/// ```
pub trait ListenerResponseEventBuilder {
    fn new(msg: ListenResponse) -> Self;
    fn msg(&self) -> ListenResponse;
}

/// Event that contains a `ListenResponse`
///
/// # Examples
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
#[derive(Event)]
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

// TODO removing / clearing listeners
// Put JoinHandle and id in a resource vec?
fn add_listener<T>(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut Client,
    project_id: String,
    target: String,
) where
    T: ListenerResponseEventBuilder + Event,
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

/// Implement this to create custom listener create events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl CreateListenerEventBuilder for CreateListenerEvent {
///     fn target(&self) -> String {
///         self.target.clone()
///     }
/// }
pub trait CreateListenerEventBuilder {
    fn target(&self) -> String;
}

/// An event holding the target for a listener
///
/// # Examples
///
/// Sending a CreateListenerEvent:
/// ```
/// fn create_listener(
///     mut listener_creator: EventWriter<CreateListenerEvent>,
/// ) {
///     let document_path = "test_collection/test_document".into();
///
///     listener_creator.send(CreateListenerEvent {
///         target: document_path,
///     });
/// }
#[derive(Event)]
pub struct CreateListenerEvent {
    pub target: String,
}

impl CreateListenerEventBuilder for CreateListenerEvent {
    fn target(&self) -> String {
        self.target.clone()
    }
}

/// Listens for events and creates Firestore listeners.
///
/// # Examples
///
/// ## Implementing:
/// ```
/// app
///     .add_event::<CreateListenerEvent>()
///     .add_event::<ListenerResponseEvent>()
///     .add_systems(Update, create_listener_event_handler::<CreateListenerEvent, ListenerResponseEvent>
///         .run_if(in_state(FirestoreState::Ready)
///     ),);
/// ```
///
/// ## Using:
/// ```
/// fn create_listener(mut ew: EventWriter<CreateListenerEvent>) {
///     ew.send(CreateListenerEvent { target: String::from("test_collection/test_document")})
/// }
pub fn create_listener_event_handler<T, R>(
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
    mut client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
) where
    T: CreateListenerEventBuilder + Event,
    R: ListenerResponseEventBuilder + Event,
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

/// Implement this to create custom listener response events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl QueryResponseEventBuilder for QueryResponseEvent {
///     fn new(query_response: QueryResponse, id: usize) -> Self {
///         QueryResponseEvent { query_response, id }
///     }
///     fn query_response(&self) -> QueryResponse {
///         self.query_response.clone()
///     }
/// }
/// ```
pub trait QueryResponseEventBuilder {
    fn new(query_response: QueryResponse, id: usize) -> Self;
    fn query_response(&self) -> QueryResponse;
}

/// Event that contains a `QueryResponse`
///
/// # Examples
/// Responding to a `QueryResponseEvent`:
/// ```
/// fn query_response_event_handler(
///     mut er: EventReader<QueryResponseEvent>,
/// ) {
///     for e in er.iter() {
///         println!("QUERY RECEIVED: {:?}", e.query_response);
///     }
/// }
/// ```
#[derive(Event)]
pub struct QueryResponseEvent {
    pub query_response: QueryResponse,
    pub id: usize,
}

impl QueryResponseEventBuilder for QueryResponseEvent {
    fn new(query_response: QueryResponse, id: usize) -> Self {
        QueryResponseEvent { query_response, id }
    }
    fn query_response(&self) -> QueryResponse {
        self.query_response.clone()
    }
}

/// Implement this to create custom query events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl RunQueryEventBuilder for RunQueryEvent {
///     fn collection_id(&self) -> String {
///         self.collection_id.clone()
///     }
///     fn limit(&self) -> Option<i32> {
///         self.limit
///     }
///     fn order_by(&self) -> (String, QueryDirection) {
///         self.order_by.clone()
///     }
///     fn parent(&self) -> String {
///         self.parent.clone()
///     }
///     fn id(&self) -> usize {
///         self.id
///     }
/// }
pub trait RunQueryEventBuilder {
    fn parent(&self) -> String;
    fn collection_id(&self) -> String;
    fn limit(&self) -> Option<i32>;
    fn order_by(&self) -> (String, QueryDirection);
    fn id(&self) -> usize;
}

/// An event holding the parameters for a Query
///
/// # Examples
/// ```
/// RunQueryEvent {
///     parent: "".into(),
///     collection_id: "test_collection".into(),
///     limit: Some(10),
///     order_by: ("price", QueryDirection::Ascending),
///     id: 1337,
/// }
#[derive(Clone, Event)]
pub struct RunQueryEvent {
    pub parent: String,
    pub collection_id: String,
    pub limit: Option<i32>,
    pub order_by: (String, QueryDirection),
    pub id: usize,
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
    fn id(&self) -> usize {
        self.id
    }
}

/// Listens for events and creates Firestore listeners.
///
/// # Examples
///
/// ## Implementing:
/// ```
/// app
///     .add_event::<RunQueryEvent>()
///     .add_event::<QueryResponseEvent>()
///     .add_systems(Update, run_query_event_handler::<RunQueryEvent, QueryResponseEvent>
///         .run_if(in_state(FirestoreState::Ready)
///     ),);
/// ```
///
/// ## Using:
/// ```
/// fn run_query(mut ew: EventWriter<RunQueryEvent>) {
///     ew.send(
///         RunQueryEvent {
///             parent: "".into(),
///             collection_id: "test_collection".into(),
///             limit: Some(10),
///             order_by: ("price", QueryDirection::Ascending),
///             id: 1337,
///         }
///     )
/// }
pub fn run_query_event_handler<T, R>(
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
    mut client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
) where
    T: RunQueryEventBuilder + Event,
    R: QueryResponseEventBuilder + Event,
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
            e.id(),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn run_query<T>(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut Client,
    project_id: String,
    parent: String,
    collection_id: String,
    limit: Option<i32>,
    order_by: (String, QueryDirection),
    id: usize,
) where
    T: QueryResponseEventBuilder + Event,
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

        while let Some(result) = res.next().await {
            match result {
                Ok(query_response) => {
                    responses.push(query_response.clone());

                    if let Some(_continuation_selector) = query_response.continuation_selector {
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
            ctx.world.send_event(T::new(response_result, id));
        })
        .await;
    });
}

// CRUD

/// Creates a Firestore document
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

/// Updates a Firestore document
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

/// Reads a Firestore document
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

/// Deletes a Firestore document
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

/// Implement this to create custom document create events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl CreateDocumentEventBuilder for CreateDocumentEvent {
///     fn new(event: Self) -> Self {
///         event
///     }
///     fn collection_id(&self) -> String {
///         self.collection_id.clone()
///     }
///     fn document_data(&self) -> HashMap<String, Value> {
///         self.document_data.clone()
///     }
///     fn document_id(&self) -> String {
///         self.document_id.clone()
///     }
///     fn id(&self) -> usize {
///         self.id
///     }
/// }
pub trait CreateDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_id(&self) -> String;
    fn collection_id(&self) -> String;
    fn document_data(&self) -> HashMap<String, Value>;
    fn id(&self) -> usize;
}

/// An event holding the parameters for a document to create
///
/// # Examples
///
/// Sending a CreateDocumentEvent:
/// ```
/// fn create_test_document(mut document_creator: EventWriter<CreateDocumentEvent>) {
///     let document_id = "test_document".to_owned();
///     let mut document_data = HashMap::new();
///     document_data.insert(
///         "test_field".to_string(),
///         Value {
///             value_type: Some(ValueType::IntegerValue(69)),
///         },
///     );
///
///     document_creator.send(CreateDocumentEvent {
///         document_id,
///         collection_id: "test_collection".into(),
///         document_data,
///         id: 0,
///     });
/// }
#[derive(Clone, Event)]
pub struct CreateDocumentEvent {
    pub document_id: String,
    pub collection_id: String,
    pub document_data: HashMap<String, Value>,
    pub id: usize,
}

impl CreateDocumentEventBuilder for CreateDocumentEvent {
    fn new(event: Self) -> Self {
        event
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
    fn id(&self) -> usize {
        self.id
    }
}

/// Implement this to create custom CreateDocumentResponse events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl CreateDocumentResponseEventBuilder for CreateDocumentResponseEvent {
///     fn new(result: DocumentResult, id: usize) -> Self {
///         CreateDocumentResponseEvent { result, id }
///     }
/// }
pub trait CreateDocumentResponseEventBuilder {
    fn new(result: DocumentResult, id: usize) -> Self;
}

/// Event that holds the result of a DocumentCreateEvent
///
/// This event is sent after a CreateDocumentEvent is consumed.
///
/// # Examples
///
/// Consuming the response event:
/// ```
/// fn create_document_response_event_handler(mut er: EventReader<CreateDocumentResponseEvent>) {
///     for e in er.iter() {
///         match e.result.clone() {
///             Ok(result) => {
///                 println!("Document created: {:?}", result)
///             }
///             Err(status) => {
///                 println!("ERROR: Document create failed: {}", status)
///             }
///         }
///     }
/// }
#[derive(Clone, Event)]
pub struct CreateDocumentResponseEvent {
    pub result: DocumentResult,
    pub id: usize,
}

impl CreateDocumentResponseEventBuilder for CreateDocumentResponseEvent {
    fn new(result: DocumentResult, id: usize) -> Self {
        CreateDocumentResponseEvent { result, id }
    }
}

/// Listens for events and creates Firestore documents
///
/// Sends a response event after the operation is completed. The creation and
/// response events are defined as generics.
///
/// # Examples
///
/// Implementing:
/// ```
/// app.add_event::<CreateDocumentEvent>()
/// .add_event::<CreateDocumentResponseEvent>()
/// .add_systems(
///     Update, create_document_event_handler::<CreateDocumentEvent, CreateDocumentResponseEvent>
///         .run_if(in_state(FirestoreState::Ready)),
/// );
pub fn create_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: CreateDocumentEventBuilder + Event + Clone,
    R: CreateDocumentResponseEventBuilder + Event + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let collection_id = e.collection_id();
        let document_id = e.document_id();
        let fields = e.document_data();
        let id = e.id();

        runtime.spawn_background_task(move |mut ctx| async move {
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
                ctx.world.send_event(R::new(result, id));
            })
            .await;
        });
    }
}

// UPDATE

/// Implement this to create custom document update events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl UpdateDocumentEventBuilder for UpdateDocumentEvent {
///     fn new(event: UpdateDocumentEvent) -> Self {
///         event
///     }
///     fn document_data(&self) -> HashMap<String, Value> {
///         self.document_data.clone()
///     }
///     fn document_path(&self) -> String {
///         self.document_path.clone()
///     }
///     fn id(&self) -> usize {
///         self.id
///     }
/// }
pub trait UpdateDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_path(&self) -> String;
    fn document_data(&self) -> HashMap<String, Value>;
    fn id(&self) -> usize;
}

/// An event holding the parameters for a document to update
///
/// # Examples
///
/// Sending an UpdateDocumentEvent:
/// ```
/// fn update_test_document(mut document_updater: EventWriter<UpdateDocumentEvent>) {
///     let document_path = "test_collection/test_document".into();
///     let mut document_data = HashMap::new();
///
///     document_data.insert(
///         "test_field".to_string(),
///         Value {
///             value_type: Some(ValueType::IntegerValue(420)),
///         },
///     );
///
///     document_updater.send(UpdateDocumentEvent {
///         document_path,
///         document_data,
///         id: 2,
///     })
/// }
#[derive(Clone, Event)]
pub struct UpdateDocumentEvent {
    pub document_path: String,
    pub document_data: HashMap<String, Value>,
    pub id: usize,
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
    fn id(&self) -> usize {
        self.id
    }
}

/// Implement this to create custom UpdateDocumentResponse events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl UpdateDocumentResponseEventBuilder for UpdateDocumentResponseEvent {
///     fn new(result: DocumentResult, id: usize) -> Self {
///         UpdateDocumentResponseEvent { result, id }
///     }
/// }
pub trait UpdateDocumentResponseEventBuilder {
    fn new(result: DocumentResult, id: usize) -> Self;
}

/// Event that holds the result of an UpdateDocumentEvent
///
/// This event is sent after an UpdateDocumentEvent is consumed.
///
/// # Examples
///
/// Consuming the response event:
/// ```
/// fn update_document_response_event_handler(
///     mut er: EventReader<UpdateDocumentResponseEvent>,
/// ) {
///     for e in er.iter() {
///         match e.result.clone() {
///             Ok(result) => {
///                 println!("Document updated: {:?}", result);
///             }
///             Err(status) => {
///                 println!("ERROR: Document update failed: {}", status)
///             }
///         }
///     }
/// }
#[derive(Clone, Event)]
pub struct UpdateDocumentResponseEvent {
    pub result: DocumentResult,
    pub id: usize,
}

impl UpdateDocumentResponseEventBuilder for UpdateDocumentResponseEvent {
    fn new(result: DocumentResult, id: usize) -> Self {
        UpdateDocumentResponseEvent { result, id }
    }
}

/// Listens for events and updates Firestore documents
///
/// Sends a response event after the operation is completed. The update and
/// response events are defined as generics.
///
/// # Examples
///
/// Implementing:
/// ```
/// app.add_event::<UpdateDocumentEvent>()
/// .add_event::<UpdateDocumentResponseEvent>()
/// .add_systems(
///     Update, update_document_event_handler::<UpdateDocumentEvent, UpdateDocumentResponseEvent>
///         .run_if(in_state(FirestoreState::Ready)),
/// );
pub fn update_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: UpdateDocumentEventBuilder + Event + Clone,
    R: UpdateDocumentResponseEventBuilder + Event + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let document_path = e.document_path();
        let fields = e.document_data();
        let id = e.id();

        runtime.spawn_background_task(move |mut ctx| async move {
            let response =
                async_update_document(&mut client, &project_id, &document_path, fields).await;

            let result = match response {
                Ok(result) => Ok(result.into_inner()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result, id));
            })
            .await;
        });
    }
}

// READ

/// Implement this to create custom document read events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl ReadDocumentEventBuilder for ReadDocumentEvent {
///     fn new(event: ReadDocumentEvent) -> Self {
///         event
///     }
///     fn document_path(&self) -> String {
///         self.document_path.clone()
///     }
///     fn id(&self) -> usize {
///         self.id
///     }
/// }
pub trait ReadDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_path(&self) -> String;
    fn id(&self) -> usize;
}

/// An event holding the parameters for a document to read
///
/// # Examples
///
/// Sending a ReadDocumentEvent:
/// ```
/// fn read_test_document(mut document_reader: EventWriter<ReadDocumentEvent>) {
///     let document_path = "test_collection/test_document".into();
///     document_reader.send(ReadDocumentEvent {
///         document_path,
///         id: 1,
///     })
/// }
#[derive(Clone, Event)]
pub struct ReadDocumentEvent {
    pub document_path: String,
    pub id: usize,
}

impl ReadDocumentEventBuilder for ReadDocumentEvent {
    fn new(event: ReadDocumentEvent) -> Self {
        event
    }
    fn document_path(&self) -> String {
        self.document_path.clone()
    }
    fn id(&self) -> usize {
        self.id
    }
}

/// Implement this to create custom ReadDocumentResponse events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl ReadDocumentResponseEventBuilder for ReadDocumentResponseEvent {
///     fn new(result: DocumentResult, id: usize) -> Self {
///         ReadDocumentResponseEvent { result, id }
///     }
/// }
pub trait ReadDocumentResponseEventBuilder {
    fn new(result: DocumentResult, id: usize) -> Self;
}

/// Event that holds the result of a ReadDocumentEvent
///
/// This event is sent after a ReadDocumentEvent is consumed.
///
/// # Examples
///
/// Consuming the response event:
/// ```
/// fn read_document_response_event_handler(mut er: EventReader<ReadDocumentResponseEvent>) {
///     for e in er.iter() {
///         match e.result.clone() {
///             Ok(result) => {
///                 println!("Document read: {:?}", result);
///             }
///             Err(status) => {
///                 println!("ERROR: Document read failed: {}", status)
///             }
///         }
///     }
/// }
#[derive(Clone, Event)]
pub struct ReadDocumentResponseEvent {
    pub result: DocumentResult,
    pub id: usize,
}

impl ReadDocumentResponseEventBuilder for ReadDocumentResponseEvent {
    fn new(result: DocumentResult, id: usize) -> Self {
        ReadDocumentResponseEvent { result, id }
    }
}

/// Listens for events and reads Firestore documents
///
/// Sends a response event after the operation is completed. The read and
/// response events are defined as generics.
///
/// # Examples
///
/// Implementing:
/// ```
/// app.add_event::<ReadDocumentEvent>()
/// .add_event::<ReadDocumentResponseEvent>()
/// .add_systems(
///     Update, read_document_event_handler::<ReadDocumentEvent, ReadDocumentResponseEvent>
///         .run_if(in_state(FirestoreState::Ready)),
/// )
pub fn read_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: ReadDocumentEventBuilder + Event + Clone,
    R: ReadDocumentResponseEventBuilder + Event + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let document_path = e.document_path();

        let id = e.id();

        runtime.spawn_background_task(move |mut ctx| async move {
            let response = async_read_document(&mut client, &project_id, &document_path).await;

            let result = match response {
                Ok(result) => Ok(result.into_inner()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result, id));
            })
            .await;
        });
    }
}

// DELETE

/// Implement this to create custom document delete events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl DeleteDocumentEventBuilder for DeleteDocumentEvent {
///     fn new(event: DeleteDocumentEvent) -> Self {
///         event
///     }
///     fn document_path(&self) -> String {
///         self.document_path.clone()
///     }
///     fn id(&self) -> usize {
///         self.id
///     }
/// }
pub trait DeleteDocumentEventBuilder {
    fn new(event: Self) -> Self;
    fn document_path(&self) -> String;
    fn id(&self) -> usize;
}

/// An event holding the parameters for a document to delete
///
/// # Examples
///
/// Sending a DeleteDocumentEvent:
/// ```
/// fn delete_test_document(mut document_deleter: EventWriter<DeleteDocumentEvent>) {
///     let document_path = "test_collection/test_document".into();
///     document_deleter.send(DeleteDocumentEvent {
///         document_path,
///         id: 3,
///     })
/// }
#[derive(Clone, Event)]
pub struct DeleteDocumentEvent {
    pub document_path: String,
    pub id: usize,
}

impl DeleteDocumentEventBuilder for DeleteDocumentEvent {
    fn new(event: DeleteDocumentEvent) -> Self {
        event
    }
    fn document_path(&self) -> String {
        self.document_path.clone()
    }
    fn id(&self) -> usize {
        self.id
    }
}

/// Implement this to create custom DeleteDocumentResponse events
///
/// # Examples
///
/// Implementing:
/// ```
/// impl DeleteDocumentResponseEventBuilder for DeleteDocumentResponseEvent {
///     fn new(result: Result<(), Status>, id: usize) -> Self {
///         DeleteDocumentResponseEvent { result, id }
///     }
/// }
pub trait DeleteDocumentResponseEventBuilder {
    fn new(result: Result<(), Status>, id: usize) -> Self;
}

/// Event that holds the result of a DeleteDocumentEvent
///
/// This event is sent after a DeleteDocumentEvent is consumed.
///
/// # Examples
///
/// Consuming the response event:
/// ```
/// fn delete_document_response_event_handler(
///     mut er: EventReader<DeleteDocumentResponseEvent>,
/// ) {
///     for e in er.iter() {
///         match e.result.clone() {
///             Ok(result) => {
///                 println!("Document deleted: {:?}", result);
///             }
///             Err(status) => {
///                 println!("ERROR: Document delete failed: {}", status)
///             }
///         }
///     }
/// }
#[derive(Clone, Event)]
pub struct DeleteDocumentResponseEvent {
    pub result: Result<(), Status>,
    pub id: usize,
}

impl DeleteDocumentResponseEventBuilder for DeleteDocumentResponseEvent {
    fn new(result: Result<(), Status>, id: usize) -> Self {
        DeleteDocumentResponseEvent { result, id }
    }
}

/// Listens for events and deletes Firestore documents
///
/// Sends a response event after the operation is completed. The delete and
/// response events are defined as generics.
///
/// # Examples
///
/// Implementing:
/// ```
/// app.add_event::<DeleteDocumentEvent>()
/// .add_event::<DeleteDocumentResponseEvent>()
/// .add_systems(
///     Update, delete_document_event_handler::<DeleteDocumentEvent, DeleteDocumentResponseEvent>
///     .run_if(in_state(FirestoreState::Ready)),
/// );
pub fn delete_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
) where
    T: DeleteDocumentEventBuilder + Event + Clone,
    R: DeleteDocumentResponseEventBuilder + Event + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let document_path = e.document_path();
        let id = e.id();

        runtime.spawn_background_task(move |mut ctx| async move {
            let response = async_delete_document(&mut client, &project_id, &document_path).await;

            let result = match response {
                Ok(_) => Ok(()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result, id));
            })
            .await;
        });
    }
}
