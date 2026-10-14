#!/usr/bin/env bash
# Step 5: 24-bit color. Should render as smooth gradients with no visible banding.
cols=${1:-72}
for row in red green blue rainbow; do
  for i in $(seq 0 $((cols - 1))); do
    v=$(( i * 255 / (cols - 1) ))
    case $row in
      red) r=$v g=0 b=0 ;; green) r=0 g=$v b=0 ;; blue) r=0 g=0 b=$v ;;
      rainbow) r=$(( 255 - v )) g=$(( v < 128 ? v * 2 : 510 - v * 2 )) b=$v ;;
    esac
    printf '\e[48;2;%d;%d;%dm \e[0m' "$r" "$g" "$b"
  done
  printf '  %s\n' "$row"
done
