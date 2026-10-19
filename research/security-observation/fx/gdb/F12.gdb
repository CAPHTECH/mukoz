# F12 witness: any access to users[i]->token (16 bytes each, two 8-byte watchpoints per user).
# awatch, not rwatch: x86 has no read-only data breakpoints, and nothing writes token after startup.
set pagination off
set confirm off
handle SIGTERM nostop noprint pass
handle SIGPIPE nostop noprint pass
awatch -l *(long *)&users[0]->token[0]
awatch -l *(long *)&users[0]->token[8]
awatch -l *(long *)&users[1]->token[0]
awatch -l *(long *)&users[1]->token[8]
commands 1-4
  printf "WITNESS_HIT\n"
  bt 3
  continue
end
printf "WATCHING\n"
continue
