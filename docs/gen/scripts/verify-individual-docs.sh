set -ex

process_directory() {
  local nix_dir=$1
  local md_dir=$2
  local category=$3

  nixFiles=($(ls $nix_dir/*.nix))
  mdFiles=($(ls $md_dir/*.md))

  declare -a nixList
  declare -a mdList

  # Remove extensions and populate lists
  for file in "''${nixFiles[@]}"; do
    baseName=$(basename "$file" .nix)
    nixList+=("$baseName")
  done

  for file in "''${mdFiles[@]}"; do
    baseName=$(basename "$file" .md)
    mdList+=("$baseName")
  done

  IFS=$'\n' sorted_nix=($(sort <<<"''${nixList[*]}"))
  IFS=$'\n' sorted_md=($(sort <<<"''${mdList[*]}"))

  # Compare and create missing files
  missing_files=()
  for item in "''${sorted_nix[@]}"; do
    if [[ ! " ''${sorted_md[@]} " =~ " $item " ]]; then
      missing_files+=("$item")
      cat <<EOF >"$md_dir/$item.md"

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
EOF
      echo "Created missing file: $md_dir/$item.md"
    fi
  done

  if [ ''${#missing_files[@]} -eq 0 ]; then
    echo "All $category docs markdown files are present."
  fi
}

process_directory "../../src/modules/languages" "../src/individual-docs/languages" "language"
process_directory "../../src/modules/services" "../src/individual-docs/services" "service"
process_directory "../../src/modules/process-managers" "../src/individual-docs/process-managers" "process manager"
