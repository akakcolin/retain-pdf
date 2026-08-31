# rendering/analysis

## 负责什么

页面和文档分析层。这里负责回答“这一页是什么情况”和“这一页建议走什么渲染路线”。

## 对外入口

- `profile/`（数据模型；profile 构建已 native-only）
- `route/`

## 不该做什么

- 不操作 PDF 内容。
- 不生成 Typst。
- 不执行 redaction。
- 不做译文 bbox 排版。
