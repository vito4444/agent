import type { AdvertisedMenus } from "../types";

type Props = {
  value: string;
  onChange: (v: string) => void;
  onSend: () => void;
  menus: AdvertisedMenus;
  permission?: { op_type: string; title: string } | null;
  onPermission: (allow: boolean) => void;
  inplaceWarning?: boolean;
  modelSwitchHint?: string | null;
};

export function Composer({
  value,
  onChange,
  onSend,
  menus,
  permission,
  onPermission,
  inplaceWarning,
  modelSwitchHint,
}: Props) {
  return (
    <div className="composer-wrap">
      {permission ? (
        <div className="permission-bar">
          <span>
            Permission · op_type=<code>{permission.op_type}</code> ·{" "}
            {permission.title}
          </span>
          <button type="button" className="primary" onClick={() => onPermission(true)}>
            Allow
          </button>
          <button type="button" onClick={() => onPermission(false)}>
            Deny
          </button>
        </div>
      ) : null}

      <div className="composer">
        <textarea
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="> 输入任务或粘贴说明（引用行风格，非气泡）"
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              onSend();
            }
          }}
        />
        <div className="composer-meta">
          {/* 未广告的 category：DOM 都不挂 */}
          {menus.model ? (
            <select
              aria-label="model"
              defaultValue={String(menus.model.currentValue ?? "")}
            >
              {menus.model.options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.name}
                </option>
              ))}
            </select>
          ) : null}
          {menus.thought_level ? (
            <select
              aria-label="thought_level"
              defaultValue={String(menus.thought_level.currentValue ?? "")}
            >
              {menus.thought_level.options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.name}
                </option>
              ))}
            </select>
          ) : null}
          <span className="ctx-chip">ctx · session</span>
        </div>
      </div>

      {modelSwitchHint ? (
        <div className="warn-inplace">{modelSwitchHint}</div>
      ) : null}
      {inplaceWarning ? (
        <div className="warn-inplace">⚠ 原地·非隔离</div>
      ) : null}

      <div style={{ marginTop: "0.5rem", display: "flex", gap: "0.5rem" }}>
        <button type="button" className="primary" onClick={onSend}>
          Send
        </button>
        <button
          type="button"
          onClick={() => {
            // Not a fourth IDE — just open path externally when possible.
            window.open("vscode://file/", "_blank");
          }}
        >
          外链打开编辑器
        </button>
      </div>
    </div>
  );
}
