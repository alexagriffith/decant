import type { SessionDetail } from "./query.ts";

export function toMarkdown(detail: SessionDetail): string {
  const summary = detail.summary;
  let out = "";
  const title = summary.title ?? summary.source_session_id;
  out += `# ${title}\n\n`;
  out +=
    `- **tool:** ${summary.tool}\n` +
    `- **model:** ${summary.model ?? ""}\n` +
    `- **messages:** ${summary.message_count}\n` +
    `- **est. cost:** $${summary.estimated_cost_usd.toFixed(2)}\n` +
    `- **started:** ${summary.started_at ?? ""}\n\n`;

  for (const message of detail.messages) {
    out += `## ${message.role.toUpperCase()}\n\n`;
    for (const block of message.blocks) {
      if (block.block_type === "text") {
        if (block.text != null) {
          out += `${block.text}\n\n`;
        }
      } else if (block.block_type === "thinking") {
        if (block.text != null) {
          out += `> _thinking:_ ${block.text}\n\n`;
        }
      } else if (block.block_type === "tool_use") {
        out += `**→ ${block.tool_name ?? ""}**\n\n`;
        out += `\`\`\`json\n${block.tool_input ?? ""}\n\`\`\`\n\n`;
      } else if (block.block_type === "tool_result") {
        out += `\`\`\`\n${block.tool_result ?? ""}\n\`\`\`\n\n`;
      } else {
        out += `_[${block.block_type}]_\n\n`;
      }
    }
  }
  return out;
}
