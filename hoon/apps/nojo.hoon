/=  *  /common/wrapper
::
=>
|%
+$  versioned-state
  $:  %v1
      ~
  ==
::
+$  effect
  $%  [%log str=@t]
  ==
::
+$  cause
  $%  [%command str=@t]
  ==
--
|%
++  moat  (keep versioned-state)
::
++  inner
  |_  state=versioned-state
  ::
  ++  load
    |=  old-state=versioned-state
    ^-  _state
    ?:  =(-.old-state %v1)
      old-state
    old-state
  ::
  ++  peek
    |=  =path
    ^-  (unit (unit *))
    ~
  ::
  ++  poke
    |=  =ovum:moat
    ^-  [(list effect) _state]
    |^
    =/  cause  ((soft cause) cause.input.ovum)
    ?~  cause
      ~>  %slog.[0 (crip "invalid cause {<cause.input.ovum>}")]
      :_  state
      [%log 'Invalid cause format']~
    ~>  %slog.[0 (cat 3 'poked: ' str.u.cause)]
    =/  subject=vase  !>(.)
    =/  program=(each (pair type nock) tang)
      (mule |.((ride p.subject str.u.cause)))
    ?:  ?=(%| -.program)
      :_  state
      [%log (tang-to-cord p.program)]~
    =/  result=(each vase tang)
      (mule |.([p.p.program .*(q.subject q.p.program)]))
    ?:  ?=(%| -.result)
      :_  state
      [%log (tang-to-cord p.result)]~
    ~>  %slog.[0 (crip (text p.result))]
    :_  state
    [%log (crip (text p.result))]~
    ::
    ++  tang-to-cord
      |=  =tang
      ^-  cord
      %+  rap  3
      %+  join  '\0a'
      ^-  wain
      %+  turn  tang
      |=  t=tank
      (rap 3 (join '\0a' (turn (wash [0 80] t) crip)))
    --
  --
--
((moat |) inner)
