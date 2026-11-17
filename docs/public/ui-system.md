# Native workbench design

`apps/native/src/style.rs` owns the base tokens and controls. The wgpu scene draws device textures, pointers and shared glass. The toolbar and composer use Iced controls.

## Workbench

The portrait phone sits left of the shared conversation. A landscape phone, computer, or linked pair sits above it. One layout transition moves the devices, cable and conversation together. Device streams, the conversation and its draft persist through focus changes.

Tools open beside their toolbar icons. Switching tools closes the previous window and its children. Audio, phone rotation and screenshot controls sit beside their device. The screenshot preview can save the image or attach it to the shared conversation.

Choose the language in a fixed 680 × 600 onboarding window. Phone and computer configuration share the same step, control dimensions and units. The workbench expands the window when the backend is ready.
