import { type RepositoryEngineKind } from "../services/projects";
import { SelectField, type SelectFieldProps } from "./Field";

const DEFAULT_ENGINE_HINT =
  "Future engines stay visible but disabled until the runtime grows a real adapter.";

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

type RepositoryEngineFieldProps = Omit<SelectFieldProps, "label" | "options"> & {
  label?: string;
};

export function RepositoryEngineField({
  hint = DEFAULT_ENGINE_HINT,
  label = "Engine",
  ...props
}: RepositoryEngineFieldProps) {
  return (
    <SelectField
      {...props}
      hint={hint}
      label={label}
      options={REPOSITORY_ENGINE_OPTIONS}
    />
  );
}