export type NormalizedEvent =
  | { kind: "message"; role: string; text: string }
  | { kind: "thought"; text: string }
  | {
      kind: "tool_call";
      tool_call_id: string;
      title: string;
      kind_name?: string;
      status?: string;
    }
  | {
      kind: "tool_call_update";
      tool_call_id: string;
      status?: string;
      diff?: string | null;
      terminal?: string | null;
    }
  | { kind: "diff"; path?: string | null; diff: string }
  | {
      kind: "permission_request";
      request_id?: string | null;
      tool_call_id?: string | null;
      op_type?: string | null;
      raw: unknown;
    }
  | { kind: "config_option_update"; options: unknown }
  | { kind: "ignored"; reason: string; raw: unknown }
  | { kind: "other"; session_update: string; raw: unknown };

export type StartupBanner = { level: string; message: string };

export type ConfigOption = {
  id: string;
  name: string;
  category?: string | null;
  type: string;
  currentValue?: unknown;
  options: { value: string; name: string; description?: string }[];
};

export type AdvertisedMenus = {
  model?: ConfigOption | null;
  thought_level?: ConfigOption | null;
  model_config: ConfigOption[];
};

export type UiBootstrap = {
  banners: StartupBanner[];
  demo_yaml: string;
  opencode_available: boolean;
  transcript: NormalizedEvent[];
  repo_root: string;
};

/** Fixture used when not running inside Tauri (vite-only). */
export const FIXTURE_TRANSCRIPT: NormalizedEvent[] = [
  { kind: "thought", text: "先看 src/lib.rs，确认 add 是否写反。" },
  { kind: "thought", text: "准备把 a-b 改成 a+b，并跑测试。" },
  { kind: "message", role: "user", text: "修 add，再补测试" },
  { kind: "message", role: "agent", text: "我会先改 src/lib.rs，再跑 gate。" },
  {
    kind: "tool_call",
    tool_call_id: "call_1",
    title: "Edit src/lib.rs",
    status: "completed",
  },
  {
    kind: "tool_call_update",
    tool_call_id: "call_1",
    status: "completed",
    diff: "@@\n-pub fn add(a:i32,b:i32)->i32{a-b}\n+pub fn add(a:i32,b:i32)->i32{a+b}\n",
    terminal: "$ cargo test add_works\nok\n",
  },
  {
    kind: "permission_request",
    request_id: "p1",
    tool_call_id: "call_1",
    op_type: "edit",
    raw: {},
  },
];
