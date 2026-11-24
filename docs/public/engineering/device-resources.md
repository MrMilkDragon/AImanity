# Device resources

A Fluorine workspace can start with an empty device list. Use Device Manager to create an Android phone or Linux desktop and select its CPU, memory and disk capacity. The workbench opens a device for control when its own connection is ready.

**Disconnect** powers down an owned guest and keeps its disk. **Start** opens the same instance again. **Delete** removes that instance and its private files. For paired physical devices, these actions end the connection or remove the pairing from the workspace.

## Android hosts

The ARM64 phone and tablet client runs one Android Virtualization Framework process per requested guest. The root manager handles authorization when the user starts a guest. Each instance owns its writable disks, control key and transport. Instances on the same host share the read-only system template. Creating the first phone extracts the Android template. Creating the first desktop extracts the Linux template.

## Device audio

Each virtual device has a separate microphone input and speaker stream. A task can send a WAV from working files to a particular guest. The microphone button in Device Manager connects the host’s physical microphone to that guest. Disconnect it before assigning the same physical input to another guest. Other guests can continue recording from their own digital inputs and playing sound.
