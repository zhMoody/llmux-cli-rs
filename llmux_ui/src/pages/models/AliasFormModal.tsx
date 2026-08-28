// 新增/编辑别名：选目标模型 → 智能匹配厂商账户 → 分组勾选 + 首选
import React, { useState, useEffect, useCallback, useMemo } from "react";
import type { AvailableModel, AliasResponse } from "@/types/model";
import type { AccountPublic } from "@/types/account";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import { useT } from "@/i18n";
import { Trash2 } from "lucide-react";

export interface AliasPayload {
  alias: string;
  target_model: string;
  vendor_id?: string;
  account_ids?: number[];
  preferred_account_id?: number;
}

interface Props {
  open: boolean;
  editing: AliasResponse | null;
  models: AvailableModel[];
  accounts: AccountPublic[];
  saving: boolean;
  onClose: () => void;
  onSubmit: (payload: AliasPayload) => void;
  /** 预填的目标模型（快速关联场景） */
  initialTarget?: string;
  /** 编辑态时删除当前别名（走外部确认流程） */
  onDelete?: () => void;
}

const labelCls = "mb-1 block text-sm font-medium text-card-foreground";

export const AliasFormModal: React.FC<Props> = ({
  open,
  editing,
  models,
  accounts,
  saving,
  onClose,
  onSubmit,
  initialTarget,
  onDelete,
}) => {
  const { t } = useT();
  const [alias, setAlias] = useState("");
  const [target, setTarget] = useState("");
  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  const [preferredId, setPreferredId] = useState("");

  // ── 共享 vendor/账号解析（发现 1/2/5/6/7/8/9 的统一出口） ──
  // 解析当前目标模型归属的厂商：
  // - 编辑态且目标模型未变更时，优先保留用户原设置的自定义 vendor（发现 2/8）
  //   —— 否则目录模型 owned_by 会覆盖自定义路由（如目标恰为已知模型时）
  // - 其余情况跟随目录模型 owned_by；owned_by 为空串或目标不在已知目录时回退
  //   editing.vendor_id（发现 8：空串也不能让兜底失效）
  const resolvedVendor = useCallback(
    (tgt: string): string | undefined => {
      const model = models.find((m) => m.id === tgt);
      if (editing && tgt === editing.target_model && editing.vendor_id) {
        return editing.vendor_id;
      }
      return model?.owned_by || editing?.vendor_id || undefined;
    },
    [models, editing],
  );

  // 可勾选/预填的账号对象：仅「归属当前 vendor 且启用」的账号，一次算出（发现 1/5/6/7）
  // 不无条件并入已绑定账号，避免把禁用/异厂商账号重新选中污染提交。
  const accountsForTarget = useCallback(
    (tgt: string): Array<AccountPublic & { id: number }> => {
      if (!tgt) return [];
      const vendor = resolvedVendor(tgt);
      if (!vendor) return [];
      return accounts.filter(
        (a): a is AccountPublic & { id: number } =>
          a.id != null && a.enabled === 1 && a.vendor_id === vendor,
      );
    },
    [accounts, resolvedVendor],
  );

  useEffect(() => {
    if (open) {
      const initTarget = editing ? editing.target_model : (initialTarget ?? "");
      setAlias(editing ? editing.alias : "");
      setTarget(initTarget);
      // 预填/初始化以「当前 vendor 且启用」的账号为准，不并入已绑定的禁用/异厂商账号（发现 1）
      const initAccounts = accountsForTarget(initTarget);
      setSelectedIds(initAccounts.map((a) => a.id));
      // 仅当 preferred 仍属于当前可选集合时才保留，避免提交已不存在的陈旧首选（发现 9）
      const preferredRaw = editing?.preferred_account_id;
      setPreferredId(
        preferredRaw != null && initAccounts.some((a) => a.id === preferredRaw)
          ? String(preferredRaw)
          : "",
      );
    }
  }, [open, editing, initialTarget, models, accounts, accountsForTarget]);

  // 当前目标模型匹配的可用账户（发现 5：不再对同一 accounts 数组重复过滤）
  const matchingAccounts = useMemo(() => accountsForTarget(target), [accountsForTarget, target]);
  const grouped = matchingAccounts.reduce<Record<string, AccountPublic[]>>((acc, a) => {
    (acc[a.vendor_id] ||= []).push(a);
    return acc;
  }, {});

  const handleTargetChange = (v: string) => {
    setTarget(v);
    // 智能预填：选目标后自动勾选匹配的启用账户
    setSelectedIds(accountsForTarget(v).map((a) => a.id));
    setPreferredId("");
  };

  const toggleAccount = (id: number) => {
    setSelectedIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const selectedAccounts = accounts.filter((a) => a.id != null && selectedIds.includes(a.id!));

  const submit = () => {
    if (!alias.trim() || !target.trim()) return;
    onSubmit({
      alias: alias.trim(),
      target_model: target.trim(),
      // 目标模型所属厂商：优先保留用户原设置的自定义 vendor（发现 2/8），而非被 owned_by 覆盖
      vendor_id: resolvedVendor(target),
      account_ids: selectedIds,
      preferred_account_id: preferredId ? Number(preferredId) : undefined,
    });
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={editing ? t("aliases.form.editTitle") : t("aliases.form.createTitle")}
      size="lg"
      footer={
        <>
          {editing && onDelete && (
            <Button variant="danger" size="sm" className="mr-auto" onClick={onDelete}>
              <Trash2 className="h-4 w-4" />
              {t("aliases.form.deleteAlias")}
            </Button>
          )}
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button loading={saving} onClick={submit}>
            {editing ? t("common.save") : t("aliases.form.createTitle")}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div>
          <label className={labelCls}>{t("aliases.form.alias")} *</label>
          <Input value={alias} onChange={setAlias} placeholder="my-gpt4" />
        </div>

        <div>
          <label className={labelCls}>{t("aliases.form.selectTarget")} *</label>
          <Select
            value={target}
            onChange={handleTargetChange}
            options={[
              // 自定义别名 target 不在已知模型列表时，补充临时 option 以免 Select 空白显示（发现 3）
              ...(target && !models.some((m) => m.id === target)
                ? [{ value: target, label: target }]
                : []),
              ...models.map((m) => ({ value: m.id, label: `[${m.owned_by}] ${m.id}` })),
            ]}
            placeholder={t("aliases.form.selectTarget")}
          />
        </div>

        {target && (
          <div>
            <label className={labelCls}>
              {t("aliases.form.bindAccounts")} ({matchingAccounts.length})
            </label>
            <p className="mb-2 text-xs text-muted-foreground">{t("aliases.form.bindHint")}</p>

            {matchingAccounts.length === 0 ? (
              <p className="text-xs text-warning-foreground">{t("aliases.form.noMatchingAccounts")}</p>
            ) : (
              <>
                <div className="mb-1 flex items-center gap-3 text-xs font-semibold">
                  <button
                    type="button"
                    className="text-primary hover:underline"
                    onClick={() => setSelectedIds(matchingAccounts.map((a) => a.id))}
                  >
                    {t("aliases.form.selectAll")}
                  </button>
                  <button type="button" className="text-muted-foreground hover:underline" onClick={() => setSelectedIds([])}>
                    {t("aliases.form.deselectAll")}
                  </button>
                </div>
                <div className="max-h-40 space-y-1 overflow-y-auto rounded-xl border border-border bg-muted/40 p-2">
                  {Object.entries(grouped).map(([vid, accts]) => (
                    <div key={vid}>
                      <div className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                        [{vid}] ({accts.length})
                      </div>
                      {accts.map((a) => (
                        <label key={a.id} className="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1 transition-colors hover:bg-muted/60">
                          <input
                            type="checkbox"
                            checked={a.id != null && selectedIds.includes(a.id!)}
                            onChange={() => a.id != null && toggleAccount(a.id!)}
                            className="h-3.5 w-3.5 rounded accent-primary"
                          />
                          <span className="text-xs text-card-foreground">{a.name}</span>
                        </label>
                      ))}
                    </div>
                  ))}
                </div>

                <div className="mt-3">
                  <label className={labelCls}>{t("aliases.form.preferredAccount")}</label>
                  <Select
                    value={preferredId}
                    onChange={setPreferredId}
                    options={selectedAccounts.map((a) => ({ value: String(a.id), label: `[${a.vendor_id}] ${a.name}` }))}
                    placeholder={t("aliases.form.preferredAccount")}
                  />
                  <p className="mt-1 text-xs text-muted-foreground">{t("aliases.form.preferredHint")}</p>
                </div>
              </>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
};
