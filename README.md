# bevy-firebase

Google Firebase integration for Bevy.

Currently only implements Google + GitHub OAuth2 and a limited subset of Firestore operations.

## Warnings

This is very very not battle-tested, and you will be trusting the plugin with API keys that can be used to rack up some serious bills if you're not careful. Check docs to see how to set up Firestore with an emulator.

Your keys will either be embedded in the distributed binary, or provided as separate files, but no matter which they will need to be essentially public. Ensure your GCP is prepared for this.

### Dependencies

Requires [`bevy-tokio-tasks`](https://crates.io/crates/bevy-tokio-tasks/0.11.0) for the tonic crate to work. Removing dependencies is a TODO, I just don't know Rust well enough yet.

## Version Compatibility

Targets Bevy `0.11.0`

## Usage

### Setting up

Create a Firebase project and note your ProjectID and client ID and Secret.

Place the client keys in a `keys.ron` in the root of your project, and add `keys.ron` to your `.gitignore`.

The `keys.ron` should be formatted as so:
```rs
{
    Github: Some(("YOUR-GITHUB-CLIENT-ID","YOUR-GITHUB-CLIENT-SECRET")),
    Google: Some(("YOUR-GOOGLE-CLIENT-ID-STRING.apps.googleusercontent.com","YOUR-GOOGLE-CLIENT-SECRET"))
}
```
> This structure will change, likely in the next release, so I'd advise against writing any tooling around it.

```rs
App::new()
    // PLUGINS
    .add_plugins(DefaultPlugins)
    // Dependency for firestore RPC to work
    .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
    .add_plugins(bevy_firebase_auth::AuthPlugin::default())
    .add_plugins(bevy_firebase_firestore::FirestorePlugin::default());
```

### Secrets + Keys

Google likes to put the required keys all over the place, with a couple of steps to set a project up. Here's a little walkthrough to get a hold of everything needed to use the plugins.

#### Creating a Firebase project

Go to [this link](https://console.firebase.google.com/) and create a project.

#### Project ID, API Key

Once you have created a project, go to Project Settings (In the Settings cog on the Firebase project console) and take note of the Project ID and Web API Key.

#### Client ID, Client Secret

We need to create an identifier to authenticate the app with Google's backend. Go [here](https://console.cloud.google.com/apis/credentials), select your project in the top left dropdown and create a new OAuth2 credential. Name it something recognisable, and make note of the Client ID and Client Secret once it is generated.

NOTE: I have only tested with Desktop clients.

## Developing

Clone repo, run `git submodule init` and `git submodule update`.

## License

Apache 2.0 or MIT at user's discretion.

