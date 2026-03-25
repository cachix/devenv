set -ex

process_directory() {
  local nix_dir=$1
  local md_dir=$2
  local gen_subdir=$3
  local category=$4

  nixFiles=($(ls $nix_dir/*.nix))
  mdFiles=($(ls $md_dir/*.md 2>/dev/null || true))

  declare -a nixList
  declare -a mdList

  for file in "${nixFiles[@]}"; do
    nixList+=("$(basename "$file" .nix)")
  done

  for file in "${mdFiles[@]}"; do
    mdList+=("$(basename "$file" .md)")
  done

  missing_files=()
  for item in "${nixList[@]}"; do
    if [[ ! " ${mdList[@]} " =~ " $item " ]]; then
      missing_files+=("$item")
      echo "--8<-- \"_generated/$gen_subdir/$item-options.md\"" > "$md_dir/$item.md"
      echo "Created missing file: $md_dir/$item.md"
    fi
  done

  if [ ${#missing_files[@]} -eq 0 ]; then
    echo "All $category doc files are present."
  fi
}

process_directory "$DEVENV_ROOT/src/modules/languages" "$DEVENV_ROOT/docs/src/languages" "languages" "language"
process_directory "$DEVENV_ROOT/src/modules/services" "$DEVENV_ROOT/docs/src/services" "services" "service"
process_directory "$DEVENV_ROOT/src/modules/process-managers" "$DEVENV_ROOT/docs/src/supported-process-managers" "supported-process-managers" "process manager"
