#!/bin/bash
set -uxo pipefail

if [ -f /cfg/coreutils ]; then
    RM="/cfg/coreutils rm"
    MKFIFO="/cfg/coreutils mkfifo"
    MKNOD="/cfg/coreutils mknod"
    LS="/cfg/coreutils ls"
    KILL="/cfg/coreutils kill"
    SLEEP="/cfg/coreutils sleep"
    ECHO="/cfg/coreutils echo"
elif [ -f /helpers/coreutils ]; then
# used for docker/podman to have a single volume sharing helper binaries
# across nodes independent from the /cfg where some files are stored
# by the node itself
    RM="/helpers/coreutils rm"
    MKFIFO="/helpers/coreutils mkfifo"
    MKNOD="/helpers/coreutils mknod"
    LS="/helpers/coreutils ls"
    KILL="/helpers/coreutils kill"
    SLEEP="/helpers/coreutils sleep"
    ECHO="/helpers/coreutils echo"
else
    RM="rm"
    MKFIFO="mkfifo"
    MKNOD="mknod"
    LS="ls"
    KILL="kill"
    SLEEP="sleep"
    ECHO="echo"
fi

echo "COMMANDS DEFINED"

# add /cfg as first `looking dir` to allow to overrides commands.
export PATH="/cfg":$PATH

echo "EXPORT PATH"

# setup pipe
pipe=/tmp/zombiepipe
trap "$RM -f $pipe" EXIT

# try mkfifo first and allow to fail
if [[ ! -p $pipe ]]; then
    $MKFIFO $pipe
fi

# set immediately exit on any non 0 exit code
set -e

# if fails try mknod
if [[ ! -p $pipe ]]; then
    $MKNOD $pipe p
fi

echo "PIPE CREATED"

# init empty
child_pid=""

# get the command to exec
CMD=($@)

# File to store CMD (and update from there)
ZOMBIE_CMD_FILE=/tmp/zombie.cmd
ZOMBIE_CMD_PID=/tmp/zombie.pid

# Store the cmd and make it available to later usage
# NOTE: echo without new line to allow to customize the cmd later
$ECHO -n "${CMD[@]}" > $ZOMBIE_CMD_FILE

echo "COMMAND TO RUN IS: $CMD"

start() {
    # redirect the output to be expored to loki
    "${CMD[@]}" >> /proc/1/fd/1 2>> /proc/1/fd/2 &
    if [[ "$CMD" != "cat" ]]; then
        child_pid="$!"

        $ECHO $(cat $ZOMBIE_CMD_FILE)
        # store pid
        $ECHO ${child_pid} > $ZOMBIE_CMD_PID

        # check if the process is running
        if ! $LS /proc/$child_pid > /dev/null 2>&1 ; then
            echo "child process doesn't exist, quiting...";
            exit 1;
        else
            echo "PID: $child_pid alive";
        fi;
    else
        echo "Process not started, PID not stored, since was 'cat'";
    fi;

}

restart() {
    if [ ! -z "${child_pid}" ]; then
        $KILL -9 "$child_pid"
    fi

    # check if we have timeout
    if [[ "$1" ]]; then
        $SLEEP "$1"
    fi

    start
}

pause() {
    if [ ! -z "${child_pid}" ]; then
        $KILL -STOP "$child_pid"
    fi
}

resume() {
    if [ ! -z "${child_pid}" ]; then
        $KILL -CONT "$child_pid"
    fi
}


# keep listening from the pipe
while read line <$pipe
echo "read line: ${line}"
do
    if [[ "$line" == "start" ]]; then
        start
    elif [[ "$line" == "quit" ]]; then
        break
    elif [[ "$line" =~ "restart" ]]; then
        # check if we have timeout between restart
        if [[ $line =~ [^0-9]+([0-9]+) ]]; then
            restart "${BASH_REMATCH[1]}"
        else
            restart 0
        fi;
    elif [[ "$line" == "pause" ]]; then
        pause
    elif [[ "$line" == "resume" ]]; then
        resume
    fi
done

exit 0
