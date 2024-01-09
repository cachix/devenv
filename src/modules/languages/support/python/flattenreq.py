import os
import re
import sys

envvar_pattern = r"\$\{([^\}]+)\}"


def replace_envvars(text, file_path):
    def replace(match):
        env_var = match.group(1)
        val = os.environ.get(env_var, None)
        if val is None:
            raise ValueError(
                f"No such environment variable {env_var} in "
                f"{text} within {file_path}"
            )
        return val

    return re.sub(envvar_pattern, replace, text)


def flatten_requirements(file_path, outdir):
    requirements = set()
    constraints = set()

    def process_file(file_path):
        if not file_path.startswith(os.path.sep):
            prefix = os.getcwd()
            file_path = os.path.join(prefix, file_path)
        with open(file_path, "r") as file:
            for line in file:
                prefix = os.path.dirname(file_path)
                line = line.strip()
                if line.startswith("-r"):
                    line = replace_envvars(line, file_path)
                    nested_file_path = re.match(r"-r\s+(.+)", line).group(1)
                    if not nested_file_path.startswith(os.path.sep):
                        nested_file_path = os.path.join(prefix, nested_file_path)
                    process_file(nested_file_path)
                elif line.startswith("-c"):
                    line = replace_envvars(line, file_path)
                    constraint_file_path = re.match(r"-c\s+(.+)", line).group(1)
                    if not constraint_file_path.startswith(os.path.sep):
                        constraint_file_path = os.path.join(
                            prefix, constraint_file_path
                        )
                    process_constraints(constraint_file_path)
                elif not line.startswith("#") and line:
                    requirements.add(line)

    def process_constraints(constraint_file_path):
        with open(constraint_file_path, "r") as file:
            for line in file:
                line = line.strip()
                if not line.startswith("#") and line:
                    constraints.add(line)

    process_file(file_path)

    with open(os.path.join(outdir, "requirements.txt"), "w") as output_file:
        for req in sorted(requirements):
            output_file.write(req + "\n")

    with open(os.path.join(outdir, "constraints.txt"), "w") as con_output_file:
        for con in sorted(constraints):
            con_output_file.write(con + "\n")


if __name__ == "__main__":
    requirements_file_path, outdir = sys.argv[1], sys.argv[2]
    if os.path.isfile(outdir):
        raise OSError(f"{outdir} is an existing file")
    os.makedirs(outdir, exist_ok=True)
    flatten_requirements(requirements_file_path, outdir)
