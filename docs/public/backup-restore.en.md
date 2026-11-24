# Back up the Fluorine workspace

[简体中文](backup-restore.zh-CN.md)

Quit the application to close its database. Copy the entire `host` folder from the [application data directory](distribution.en.md) into private storage. Keep `workspace.redb` and `files/` together. The database includes conversations, model credentials, knowledge and original attachments. The folder also holds editable working files.

To restore into a fresh Fluorine profile, quit Fluorine and put the saved `host` folder in that profile's application data directory. Reopen the client and check the conversation, model settings and working files. This procedure covers the native workspace. Device disks stored elsewhere require a separate copy.
