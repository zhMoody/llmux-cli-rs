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

  // 根据目标模型算出匹配的启用账户 id（智能预填用）。
  // 自定义别名目标模型可能不在已知模型列表，只有别名上存的 vendor_id 才是权威依据，
  // 因此并入 editing.vendor_id 与已绑定账户的厂商，避免编辑自定义别名时自己的账户"消失"。
  const idsForTarget = useCallback(
    (tgt: string): number[] => {
      if (!tgt) return [];
      const vendors = new Set<string>();
      for (const m of models) if (m.id === tgt && m.owned_by) vendors.add(m.owned_by);
      if (editing?.vendor_id) vendors.add(editing.vendor_id);
      for (const a of editing?.accounts ?? []) if (a.vendor_id) vendors.add(a.vendor_id);
      return accounts
        .filter(
          (a) =>
            (vendors.has(a.vendor_id) && a.enabled) ||
            (a.id != null && (editing?.accounts?.some((ba) => ba.id === a.id) ?? false)),
        )
        .map((a) => a.id)
        .filter((id): id is number => id != null);
    },
    [models, accounts, editing],
  );

  useEffect(() => {
    if (open) {
      const initTarget = editing ? editing.target_model : (initialTarget ?? "");
      setAlias(editing ? editing.alias : "");
      setTarget(initTarget);
      setSelectedIds(editing ? editing.accounts.map((a) => a.id) : idsForTarget(initTarget));
      setPreferredId(editing?.preferred_account_id ? String(editing.preferred_account_id) : "");
    }
  }, [open, editing, initialTarget, models, accounts, idsForTarget]);

  // 可选账户 = idsForTarget 命中的账户（与预填一致，含别名已绑定账户）
  const matchingAccounts = useMemo(() => {
    const ids = new Set(idsForTarget(target));
    return accounts.filter((a) => a.id != null && ids.has(a.id!));
  }, [accounts, idsForTarget, target]);
  const grouped = matchingAccounts.reduce<Record<string, AccountPublic[]>>((acc, a) => {
    (acc[a.vendor_id] ||= []).push(a);
    return acc;
  }, {});

  const handleTargetChange = (v: string) => {
    setTarget(v);
    // 智能预填：选目标后自动勾选匹配的启用账户
    setSelectedIds(idsForTarget(v));
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
      // 目标模型所属厂商；编辑自定义别名时以其存储的 vendor_id 兜底，避免覆盖成 NULL
      vendor_id: models.find((m) => m.id === target)?.owned_by ?? editing?.vendor_id ?? undefined,
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
            options={models.map((m) => ({ value: m.id, label: `[${m.owned_by}] ${m.id}` }))}
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
                    onClick={() =>
                      setSelectedIds(matchingAccounts.map((a) => a.id).filter((id): id is number => id != null))
                    }
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
