```text
        ----<  ASTRA  >----
               \        /
                \  *  /
          *      \   /      *
                  \ /
                   +
         .                  .
                *      .

============================

  GGGG  PPPP  TTTTT       666
 G      P   P   T        6
 G  GG  PPPP    T   ---  6666
 G   G  P       T        6  6
  GGGG  P       T         666

============================

   A    SSS  TTTTT RRRR    A
  A A  S       T   R  R   A A
 AAAAA  SSS    T   RRRR  AAAAA
 A   A     S   T   R R   A   A
 A   A SSSS    T   R  R  A   A

----------------------------

        [ AVAILABLE NOW ]

----------------------------

          THINK DEEPER.
          REACH FURTHER.

               *
              /|\
             / | \
            /  |  \
           /___|___\
               |
               |
              / \
             .   .
            .     .

============================
     N E X T   I S   N O W
============================
```

# Codex VL 0.153.2-vl.2 — cell identity guard and model-message caps

Fleet TUIs no longer attach to a shared app-server without a verified cell identity:
unverified sessions use an embedded fallback with a visible diagnostic, while explicit
shared endpoints are rejected. Catalog-supplied `persistent_instructions` and Guardian
`node_repl_policy` are capped at 8 KiB by bytes at every ingress and consumer; values
over the limit are rejected rather than truncated, and Guardian fails closed with a
clear diagnostic. V8 release workflow guards, hosted SDK runners, refreshed zlib
snapshot URLs, README ASCII cleanup, and a dedicated CI gate are included.
