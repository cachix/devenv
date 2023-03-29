# Garbage collection

`devenv` involves optimizing disk space utilization through the creation of garbage collection roots for each activated developer environment, which is especially beneficial when switching between branches. 

This is based on the premise that disk space is inexpensive and can be better utilized by creating a root for each environment. The garbage collection process can be initiated by running the command `devenv gc` when it is deemed necessary to free up space.

Running ``devenv gc`` will go through everything you've built so far
and delete anything that's currently not the latest successful invocation
of any ``devenv`` command per folder.
