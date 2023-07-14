# bevy-firebase

Google Firebase integration for Bevy.

Currently only implements Auth and a limited subset of Firestore operations.

## Warnings

This is very very not battle-tested, and you will be trusting the plugin with API keys that can be used to rack up some serious bills if you're not careful. Check docs to see how to set up Firestore with an emulator. There is currently no Auth emulator support.

Your keys will either be embedded in the distributed binary, or provided as separate files, but no matter which they will need to be essentially public. Ensure your GCP is prepared for this.

## Installing

<!-- TODO: `cargo add bevy-firebase` -->
I'll get this on crates.io once I've learned CI/CD. And learned crates.io. For now it's a clone and paste job, sorry!

### Dependencies

Requires `bevy-tokio-tasks` for the tonic crate to work. Removing dependencies is a TODO, I just don't know Rust well enough yet.

## Version Compatibility

Targets Bevy `0.10.1`

`0.11.x` coming soon.

## Usage

### Setting up

Create a Firebase project, grab your keys, and feed them to the plugin like so:

```rs
    App::new()
        // PLUGINS
        .add_plugins(DefaultPlugins)
        // Dependency for firestore RPC to work
        .add_plugin(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_plugin(bevy_firebase_auth::AuthPlugin {
            firebase_project_id: "YOUR-PROJECT-ID".into(),
            google_client_id: "YOUR-CLIENT-ID".into(),
            google_client_secret: "YOUR-CLIENT-SECRET".into(),
            ..Default::default()
        })
        .add_plugin(bevy_firebase_firestore::FirestorePlugin::default());
```

### Secrets + Keys

Google likes to put the required keys all over the place, with a couple of steps to set a project up. We'll be bouncing around GCP and Firebase.

#### Creating a Firebase project

Go to [this link](https://console.firebase.google.com/) and create a project.

#### Project ID, API Key

Once you have created a project, go to Project Settings (In the Settings cog on the Firebase project console) (TODO image), and take note of the Project ID and Web API Key.

#### Client ID, Client Secret

We need to create an identifier to authenticate the app with Google's backend. Go [here](https://console.cloud.google.com/apis/credentials), select your project in the top left dropdown (TODO image), and create a new OAuth2 credential. Name it something recognisable, and make note of the Client ID and Client Secret once it is generated.

NOTE: I have only tested with Desktop clients.