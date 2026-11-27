use super::*;
use std::{env, path::PathBuf};

#[path = "window.rs"]
mod window;

const NOTE: &str = "Fluorine overlay qualification";
const DEVICES: [(&str, &str); 2] = [("overlay-phone", "phone"), ("overlay-computer", "desktop")];

#[tokio::test]
#[ignore = "opens signed baseline and candidate installers in a disposable profile"]
async fn fluorine_overlay() -> Result<()> {
    let phase = env::var("AIMANITY_QA_PHASE")?;
    let root = PathBuf::from(env::var("AIMANITY_QA_ROOT")?);
    let exe = PathBuf::from(env::var("AIMANITY_QA_CLIENT")?);
    let profile = root.join("profile");
    let home = profile.join("dev.aimanity.workspace/host");
    fs::create_dir_all(&profile)?;
    if phase == "baseline" {
        let mut app =
            window::Client::launch(&exe, &profile, &root, &root.join("baseline-new.log")).await?;
        app.choose_english().await?;
        app.capture_workbench("baseline-new.png").await?;
        app.close().await?;
        let workspace = Workspace::open(&home)?;
        for (id, kind) in DEVICES {
            workspace.dispatch(
                "registry/device-save",
                &wire::DeviceSaveRequest {
                    id: id.into(),
                    name: id.into(),
                    kind: kind.into(),
                    ..Default::default()
                }
                .encode_to_vec(),
            )?;
            fs::write(workspace.device_home(id).join("retained.txt"), id)?;
        }
        let tx = workspace.db.begin_write()?;
        let mut state: wire::RegistryState = read(&tx.open_table(RECORDS)?, "registry")?;
        state.active_agent_id = "offline".into();
        state.agents.push(wire::RegisteredAgent {
            id: "offline".into(),
            model_connection_id: "local".into(),
            ..Default::default()
        });
        put(&tx, "registry", &state)?;
        put(
            &tx,
            "model/local",
            &wire::ModelDraft {
                model: "overlay-fixture".into(),
                provider: "compatible".into(),
                protocol: "chat-completions".into(),
                base_url: "http://127.0.0.1:9/v1".into(),
                ..Default::default()
            },
        )?;
        tx.commit()?;
        let note = workspace.upload("overlay.txt".into(), NOTE.as_bytes().to_vec())?;
        let sent = workspace.submit(wire::ChatSendRequest {
            request_id: "overlay-message".into(),
            text: NOTE.into(),
            attachment_ids: vec![note.id],
            ..Default::default()
        })?;
        workspace.control(
            wire::ChatRunRequest {
                run_id: "overlay-message".into(),
                ..Default::default()
            },
            false,
        )?;
        fs::write(root.join("conversation.txt"), sent.conversation_id)?;
    } else {
        assert_eq!(phase, "candidate");
    }
    let mut app =
        window::Client::launch(&exe, &profile, &root, &root.join(format!("{phase}.log"))).await?;
    app.capture_workbench(&format!("{phase}.png")).await?;
    app.close().await?;
    let workspace = Workspace::open(&home)?;
    let state = workspace.registry()?;
    assert_eq!(state.active_agent_id, "offline");
    let model: wire::ModelDraft = workspace.get("model/local")?;
    assert_eq!(model.model, "overlay-fixture");
    for (id, kind) in DEVICES {
        assert!(state.devices.iter().any(|d| d.id == id && d.kind == kind));
        assert_eq!(
            fs::read_to_string(workspace.device_home(id).join("retained.txt"))?,
            id
        );
    }
    let history = workspace.history(wire::ChatHistoryRequest {
        limit: 80,
        ..Default::default()
    })?;
    assert_eq!(
        history.conversation_id,
        fs::read_to_string(root.join("conversation.txt"))?
    );
    let saved: Vec<_> = history
        .messages
        .iter()
        .filter(|m| m.id == "overlay-message")
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].text, NOTE);
    assert_eq!(
        workspace.attachment(&saved[0].attachments[0].id)?,
        NOTE.as_bytes()
    );
    assert_eq!(fs::read_to_string(home.join("user/native-locale"))?, "en");
    println!(
        "{phase}: matching conversation ID, one message with attachment, model, two device registrations and both device file directories"
    );
    Ok(())
}
