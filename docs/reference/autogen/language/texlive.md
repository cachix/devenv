  # Texlive
  


## languages\.texlive\.enable



Whether to enable TeX Live\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.texlive\.packages



Packages available to TeX Live



*Type:*
non-empty (list of string)



*Default:*

```
[
  "collection-basic"
]
```



## languages\.texlive\.base

TeX Live package set to use



*Type:*
unspecified value



*Default:*
` pkgs.texlive `
