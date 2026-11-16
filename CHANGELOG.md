# Changelog

## 0.0.3 Calcium

- Resume an interrupted task from its saved goal, input and tool receipts once both devices are ready.
- Reconnect the workbench after a chat stream disconnects and show the saved goal status.
- Check PostgreSQL and Redis sooner during startup.
- Reduce database requests before model calls and when retrieving learned app skills.
- Read Android boot and root status in one ADB call, without blocking device control.
- Send Linux desktop observations as protobuf bytes, reuse unchanged Android frames on Windows, and scan desktop JPEG data once across pipe reads.
- Remove repeated model request copies and keep chat history stable while typing.
- Start Copilot through the native speech permissions, open Agent settings next to the toolbar, and unify loading animations.

## 0.0.2 Beryllium

- Show live installation commands and prepare the local runtime in one extraction pass.
- Open the Agent form next to its add button.
- Simplify fresh Android home screens and manage proxy subscriptions from the workbench.
- Check for signed updates during startup and show the version and release notes in the top right.
- Resume the prepared local environment on reopen and stop the app's devices and services on quit.
- Add inline images and bidirectional links to the knowledge base. The Agent can attach verified device screenshots and update existing documents.

## 0.0.1 Aluminum

- Set up the interface language, device resources and model in order.
- Share spring range controls across setup and device volume.
- Operate Android and Linux, connect devices, and exchange files through the local workbench.
- Save multiple model connections and assign a model to each device.
- Check and repair managed runtime files when reopening the client.
- Reduce repeated work in startup polling, chat history, device audio, live screen framing, and model response streams.
