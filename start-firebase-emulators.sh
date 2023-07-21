#!/bin/bash
{
    firebase emulators:start --project demo-bevy
} || {
    echo "Install Firebase emulators first: https://firebase.google.com/docs/emulator-suite/install_and_configure"
}
