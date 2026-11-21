# Built-in capabilities, private data and boundaries

## 开箱内置能力 / Built-ins

| Skill | Purpose |
| --- | --- |
| phone-use | Observe and act on the real Android guest, native targets/OCR, bounded gestures and result checks |
| computer-use | Screenshot-based Linux interaction, keyboard/pointer and region zoom |
| device-transfer | Linked-device file exchange with direction/path/hash checks |
| wechat | Identify conversations/notifications, authorized replies, reading scope, account restrictions and honest send states |
| autonomous-learning | Evidence-backed app experience, independent audit, revision/withdrawal and avoiding repetitive empty exploration |
| browser-android | Address bar navigation, submit-vs-suggestion distinction, OCR limitations and actual loading checks |
| files-android | DocumentsUI type filters, uncertain scope, missing viewer handling and transfer handoff |

The browser and file Skills came from real Android runs and independent review. Their versioned sources contain reusable workflows with semantic targets. Startup installs built-ins from source and preserves their enabled state. User-installed and learned Skills retain separate identities.

知识库初始为空。操作知识由内置 Skill 提供，用户积累的经验保存在自己的服务器。

## Development and design Skills

Open Skills and search for a name or a task, such as `typography`, `Postgres` or `营销小字`. The same catalogue is available to the Agent through `skill_list(query)`. Read a guide with `skill_read(name)` and a linked reference with `skill_read(name, resource)`.

For a form redesign, start with `interface-copy-layout` to decide which labels to keep, align the fields and check the result at a short window height. For a slow query, use `postgres-patterns` to compare plans for the same data and parameters. The library also covers motion, code review, browser verification and creating games and visual assets. The guides and bundled references are available offline.

在「Skill」里可按名称或任务搜索。界面设计先用 `interface-copy-layout` 审查文案、基线、控件高度和字号；后端和数据库任务可分别读取 `backend-patterns`、`postgres-patterns`。Agent 按当前任务查找和读取技能。

## 数据 / Data

Model keys, revisions, chat, device disks, contacts, channel secrets, memory, attachments and screenshots are private user state. Model API requests necessarily send relevant text/images/tool results to the configured provider; selected channels receive authorized replies and screenshots. [Data and privacy](../../PRIVACY.md)

Android and Linux share one active Agent and conversation. Android root and Linux sudo apply inside their respective guests. Host desktop control requires enabling it in Settings and authorizing the task. Device profiles set the hardware model and display properties.

## 使用边界 / Responsible use

Use authorized devices and accounts. The account owner sets the scope for incoming content, outgoing messages, app installation and autonomous actions.

The action log shows targets and delivery results. Use the device controls to take over.
