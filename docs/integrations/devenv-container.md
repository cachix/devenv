You can use the [devenv container](https://github.com/cachix/devenv/pkgs/container/devenv%2Fdevenv)
to run devenv commands
on your preferred container-based system.

Any container-based environment
like Gitlab CI, Kubernetes, Docker, is supported.

=== "Docker"

    ```bash
    docker run ghcr.io/cachix/devenv/devenv:latest devenv shell hello-world
    ```

=== "GitLab CI"

    ```yaml
    devenv-job:
      image: ghcr.io/cachix/devenv/devenv:latest
      script: devenv shell hello-world
    ```

=== "Kubernetes"

    ```yaml
    apiVersion: batch/v1
    kind: Job
    metadata:
      name: devenv-job
    spec:
      template:
        spec:
          containers:
            - name: devenv-job
              image: ghcr.io/cachix/devenv/devenv:latest
              command: ["devenv", "tasks", "run", "my-app:hello-world"]
          restartPolicy: Never
      backoffLimit: 4
    ```
