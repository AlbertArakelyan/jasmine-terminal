#!/usr/bin/env bash
# Step 5: SGR text attributes, one per line, each labelled with what it should look like.
printf '\e[1mbold\e[0m             (bold face or brighter color)\n'
printf '\e[2mdim\e[0m              (faded)\n'
printf '\e[3mitalic\e[0m           (italic face)\n'
printf '\e[4munderline\e[0m        (underlined)\n'
printf '\e[7minverse\e[0m          (fg/bg swapped)\n'
printf '\e[8mhidden\e[0m           (<- should be blank)\n'
printf '\e[9mstrikethrough\e[0m    (line through)\n'
printf '\e[1;3mbold italic\e[0m      (bold-italic face)\n'
printf '\e[31mred \e[91mbright red \e[1;31mbold red\e[0m\n'
