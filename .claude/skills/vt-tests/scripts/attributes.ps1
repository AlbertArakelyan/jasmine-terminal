# Step 5: SGR text attributes, one per line, each labelled with what it should look like.
$e = [char]27
Write-Host "$e[1mbold$e[0m             (bold face)"
Write-Host "$e[2mdim$e[0m              (faded)"
Write-Host "$e[3mitalic$e[0m           (italic face)"
Write-Host "$e[4munderline$e[0m        (underlined)"
Write-Host "$e[7minverse$e[0m          (fg/bg swapped)"
Write-Host "$e[8mhidden$e[0m           (<- should be blank)"
Write-Host "$e[9mstrikethrough$e[0m    (line through)"
Write-Host "$e[1;3mbold italic$e[0m      (bold-italic face)"
Write-Host "$e[31mred $e[91mbright red $e[1;31mbold red$e[0m"
Write-Host "tab:`ta`tbc`tdef`t|   (a, bc, def, | start on tab stops 8/16/24/32)"
Write-Host "12345678901234567890123456789012"
Write-Host "$e[4;31mred$e[32mgreen$e[34mblue$e[0m   (one unbroken underline)"
