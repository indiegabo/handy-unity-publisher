import { type RepositoryEngineKind } from "../services/projects";
import { useLocalization } from "../LocalizationProvider";
import { SelectField, type SelectFieldProps } from "./Field";

export const REPOSITORY_ENGINE_OPTIONS: ReadonlyArray<{
  disabled?: boolean;
  label: string;
  value: RepositoryEngineKind;
}> = [
  { label: "Unity", value: "unity" },
  { label: "Unreal", value: "unreal", disabled: true },
  { label: "Godot", value: "godot", disabled: true },
  { label: "GameMaker", value: "gamemaker", disabled: true },
  { label: "Defold", value: "defold", disabled: true },
  { label: "Cocos Creator", value: "cocos-creator", disabled: true },
];

type RepositoryEngineFieldProps = Omit<
  SelectFieldProps,
  "label" | "options"
> & {
  label?: string;
};

export function RepositoryEngineField({
  hint,
  label,
  ...props
}: RepositoryEngineFieldProps) {
  const { t } = useLocalization();

  return (
    <SelectField
      {...props}
      hint={
        hint ??
        t(
          "project_shared.engine.hint",
          "Future engines stay visible but disabled until the runtime grows a real adapter.",
        )
      }
      label={label ?? t("project_shared.engine.label", "Engine")}
      options={[
        {
          label: t("project_shared.engine.option.unity", "Unity"),
          value: "unity",
        },
        {
          disabled: true,
          label: t("project_shared.engine.option.unreal", "Unreal"),
          value: "unreal",
        },
        {
          disabled: true,
          label: t("project_shared.engine.option.godot", "Godot"),
          value: "godot",
        },
        {
          disabled: true,
          label: t("project_shared.engine.option.gamemaker", "GameMaker"),
          value: "gamemaker",
        },
        {
          disabled: true,
          label: t("project_shared.engine.option.defold", "Defold"),
          value: "defold",
        },
        {
          disabled: true,
          label: t(
            "project_shared.engine.option.cocos_creator",
            "Cocos Creator",
          ),
          value: "cocos-creator",
        },
      ]}
    />
  );
}
