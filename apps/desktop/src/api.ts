import type { AdvertisedMenus, NormalizedEvent, UiBootstrap } from "./types";
import { FIXTURE_TRANSCRIPT } from "./types";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(cmd, args);
  } catch {
    throw new Error("not-tauri");
  }
}

export async function loadBootstrap(): Promise<UiBootstrap> {
  try {
    return await invoke<UiBootstrap>("bootstrap");
  } catch {
    return {
      banners: [
        {
          level: "warn",
          message:
            "未在 Tauri 壳内运行：使用 fixture 回放。桌面打包见 README。",
        },
      ],
      demo_yaml: `name: fix-and-test
tasks:
  - id: A
    title: Fix add function
    prompt: Fix add
    gate_command: "test -f src/lib.rs"
    produces:
      - id: fixed_lib
        path: src/lib.rs
  - id: B
    title: Write tests
    prompt: Write tests
    produces:
      - id: extra_tests
        path: tests/extra.rs
deps:
  - from: A
    to: B
    artifacts:
      - id: fixed_lib
        path: src/lib.rs
`,
      opencode_available: false,
      transcript: FIXTURE_TRANSCRIPT,
      repo_root: "(fixture)",
    };
  }
}

export async function runMockGraph(yaml: string) {
  return invoke<unknown>("run_mock_graph", { yaml });
}

export async function memorySnapshot() {
  try {
    return await invoke<{
      l0: { id: string; content: string }[];
      l1: { id: string; content: string; invalid_at?: string | null }[];
      proposals: { id: string; content: string; status: string }[];
      l2: { id: string; content: string }[];
    }>("memory_snapshot");
  } catch {
    return {
      l0: [{ id: "fixture-l0", content: "只读规则：修改代码前必须先写失败测试。" }],
      l1: [
        {
          id: "fixture-l1",
          content: "mock: add() 曾经返回 a-b",
          invalid_at: null,
        },
      ],
      proposals: [
        {
          id: "fixture-p",
          content: "ACE假提案：优先用属性测试覆盖加法交换律",
          status: "pending",
        },
      ],
      l2: [] as { id: string; content: string }[],
    };
  }
}

export async function approveProposal(id: string) {
  return invoke("approve_proposal", { id });
}

export async function invalidateL1(id: string) {
  return invoke("invalidate_l1", { id });
}

export async function loadMenus(): Promise<AdvertisedMenus> {
  try {
    return await invoke<AdvertisedMenus>("advertised_menus_fixture");
  } catch {
    // Empty on purpose — unadvertised categories must not mount.
    return { model: null, thought_level: null, model_config: [] };
  }
}

export async function getTranscript(): Promise<NormalizedEvent[]> {
  try {
    return await invoke<NormalizedEvent[]>("get_transcript");
  } catch {
    return FIXTURE_TRANSCRIPT;
  }
}
