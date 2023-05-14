Login:

```
flyctl auth login
```

Create an app:

```
flyctl apps create simple-python-app
```

Allocate ipv4:

```
flyctl ips allocate-v4
```

Copy the container to fly.io registry:

```
devenv container processes --copy 
```

Create a volume for `devenv` state:

```
fly volumes create devenv_state --region ams --size 1
```

Deploy your app:

```
flyctl deploy
```