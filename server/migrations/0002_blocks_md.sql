-- blocks 增加 md 列：块级 markdown（docx / 富文本 / Viewer 文本视图的输入）。
-- text 列保留纯文本（剪贴板纯文本路径）。

ALTER TABLE blocks ADD COLUMN md TEXT NOT NULL DEFAULT '';
