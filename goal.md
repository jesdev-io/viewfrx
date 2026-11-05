# `frxview`: A lighnting fast audio file browser

## Goal
The goal of this app is to be a vibe-coded, backed-by-reference, super fast, compact, easy to use app for browsing and listening to audio files (`.wav`, `.mp3`, ...) written in **rust**. Why is this needed? Because in scenarios where lots of data is browsed according to human metrics such as personal taste, speed is key: Imagine a gignatic database of field recordings or foley sounds. To find a fitting audio scene, a human curator needs to be able to quickly get the gist of the sound both in terms of technical and meta data and in feel. The second can be provided by super fast, immideate playback and super responsive jumps along the audio file, proper speed-up implementation and an overall snappy feel.

## App windows
The App does not need much. But what it has, should be concise and fast. Audio files or folders can be imported, only audio files are shown if a folder is selected. This file browsing window is to the left. Metadata is shown in the upper center, while the rest of the middle row has space for the audio waveform, the audio control buttons and the timeline in general, where the play bar is located. To the right, there are playback settings and a live VU meter, optionally a configurable magnitude spectrum. Additionally, a spectrogram can be shown above the timeline if needed, similar to audacity. Gain can be manipulated for playback and metadata can be added if desired.

## Performance
To ensure good performance, we need proper caching and preloading. The worst that can happen is for the app to be laggy or freeze. This is not allowed to happen. Some form of multithreading is mandatory. The system's audio devices and drivers need to be recognized and should be configurable in the settings.

## Theme
Find the theme, logo and styling in ../jesdev/jesdev-io-website.

## References
This project is inspired by the speed and rust writeup of the ArtCraft project. Look that up. Also, consider Audacity as a reference. Additionally, look up what other audio library browsers are doing. Why do they exist? What features do they have? How were they made fast? If you need a proper code scan, clone the repos temporarily. 

## Project
Use git locally here. A remote has not yet been set up, this will come later. Try to target linux, windows and macos.

## Key to success
ArtCraft is so succesful because it exposes everything in the app as CLI. DO the same. This way, you can write unit tests. test, test, test. A test fails? Dont just hardcode the test to succeed. Check what caused the issue in the first place. Keep the codebase tight. Reuse code. Test. Run. Check speed performance. Check responsiveness. A sold CLI indicated that we are 90% of the way there. Do not crate spaghetti code where backend and frontend are mixed. The program has to run entirely on CLI, only then can we create a UI layer on top of that.

**Leave this file untouched!**