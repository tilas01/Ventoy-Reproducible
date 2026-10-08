#!/ventoy/busybox/sh

. /ventoy/hook/ventoy-hook-lib.sh

vtlog "####### $0 $* ########"

ventoy_copy_udev_auto_rules

$BUSYBOX_PATH/true
