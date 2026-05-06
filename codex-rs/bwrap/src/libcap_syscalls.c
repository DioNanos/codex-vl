#include <stddef.h>
#include <strings.h>
#include <sys/capability.h>
#include <sys/syscall.h>
#include <unistd.h>

struct cap_name {
  const char *name;
  cap_value_t value;
};

static const struct cap_name CAP_NAMES[] = {
  {"chown", CAP_CHOWN},
  {"dac_override", CAP_DAC_OVERRIDE},
  {"dac_read_search", CAP_DAC_READ_SEARCH},
  {"fowner", CAP_FOWNER},
  {"fsetid", CAP_FSETID},
  {"kill", CAP_KILL},
  {"setgid", CAP_SETGID},
  {"setuid", CAP_SETUID},
  {"setpcap", CAP_SETPCAP},
  {"linux_immutable", CAP_LINUX_IMMUTABLE},
  {"net_bind_service", CAP_NET_BIND_SERVICE},
  {"net_broadcast", CAP_NET_BROADCAST},
  {"net_admin", CAP_NET_ADMIN},
  {"net_raw", CAP_NET_RAW},
  {"ipc_lock", CAP_IPC_LOCK},
  {"ipc_owner", CAP_IPC_OWNER},
  {"sys_module", CAP_SYS_MODULE},
  {"sys_rawio", CAP_SYS_RAWIO},
  {"sys_chroot", CAP_SYS_CHROOT},
  {"sys_ptrace", CAP_SYS_PTRACE},
  {"sys_pacct", CAP_SYS_PACCT},
  {"sys_admin", CAP_SYS_ADMIN},
  {"sys_boot", CAP_SYS_BOOT},
  {"sys_nice", CAP_SYS_NICE},
  {"sys_resource", CAP_SYS_RESOURCE},
  {"sys_time", CAP_SYS_TIME},
  {"sys_tty_config", CAP_SYS_TTY_CONFIG},
  {"mknod", CAP_MKNOD},
  {"lease", CAP_LEASE},
  {"audit_write", CAP_AUDIT_WRITE},
  {"audit_control", CAP_AUDIT_CONTROL},
  {"setfcap", CAP_SETFCAP},
  {"mac_override", CAP_MAC_OVERRIDE},
  {"mac_admin", CAP_MAC_ADMIN},
  {"syslog", CAP_SYSLOG},
  {"wake_alarm", CAP_WAKE_ALARM},
  {"block_suspend", CAP_BLOCK_SUSPEND},
  {"audit_read", CAP_AUDIT_READ},
#ifdef CAP_PERFMON
  {"perfmon", CAP_PERFMON},
#endif
#ifdef CAP_BPF
  {"bpf", CAP_BPF},
#endif
#ifdef CAP_CHECKPOINT_RESTORE
  {"checkpoint_restore", CAP_CHECKPOINT_RESTORE},
#endif
};

int cap_from_name(const char *name, cap_value_t *value)
{
  if (name == NULL || value == NULL) {
    return -1;
  }

  if (strncasecmp(name, "cap_", 4) == 0) {
    name += 4;
  }

  for (size_t i = 0; i < sizeof(CAP_NAMES) / sizeof(CAP_NAMES[0]); i++) {
    if (strcasecmp(name, CAP_NAMES[i].name) == 0) {
      *value = CAP_NAMES[i].value;
      return 0;
    }
  }

  return -1;
}

int capget(cap_user_header_t header, cap_user_data_t data)
{
  return syscall(SYS_capget, header, data);
}

int capset(cap_user_header_t header, const cap_user_data_t data)
{
  return syscall(SYS_capset, header, data);
}
