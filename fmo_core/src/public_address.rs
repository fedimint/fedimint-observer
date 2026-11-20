//! Which IP addresses are public: the policy for every outbound connection to
//! an address learned from guardians, gateways or invite codes.
use std::net::IpAddr;

/// IPv4 blocks a probe must not reach: the IANA IPv4 Special-Purpose Address
/// Registry entries (RFC 6890 and updates), multicast and reserved space.
/// Blocks with a few globally reachable anycast addresses are excluded whole.
const NON_PUBLIC_V4: &[(u32, u32)] = &[
    (0x0000_0000, 8),  // 0.0.0.0/8 this network
    (0x0A00_0000, 8),  // 10.0.0.0/8 private
    (0x6440_0000, 10), // 100.64.0.0/10 shared address space
    (0x7F00_0000, 8),  // 127.0.0.0/8 loopback
    (0xA9FE_0000, 16), // 169.254.0.0/16 link local
    (0xAC10_0000, 12), // 172.16.0.0/12 private
    (0xC000_0000, 24), // 192.0.0.0/24 IETF protocol assignments
    (0xC000_0200, 24), // 192.0.2.0/24 documentation
    (0xC058_6300, 24), // 192.88.99.0/24 deprecated 6to4 relay anycast
    (0xC0A8_0000, 16), // 192.168.0.0/16 private
    (0xC612_0000, 15), // 198.18.0.0/15 benchmarking
    (0xC633_6400, 24), // 198.51.100.0/24 documentation
    (0xCB00_7100, 24), // 203.0.113.0/24 documentation
    (0xE000_0000, 4),  // 224.0.0.0/4 multicast
    (0xF000_0000, 4),  // 240.0.0.0/4 reserved, including broadcast
];

/// Within global unicast 2000::/3, the IANA IPv6 Special-Purpose Address
/// Registry blocks that are not public hosts, or that embed or tunnel IPv4.
/// Everything outside 2000::/3 (loopback, mapped, NAT64, discard, ULA, link
/// local, multicast, SRv6 SIDs, ...) is rejected before this table.
const NON_PUBLIC_V6: &[(u128, u32)] = &[
    (0x2001_0000_u128 << 96, 23), /* 2001::/23 IETF protocol assignments: Teredo, benchmarking,
                                   * ORCHIDv2 */
    (0x2001_0db8_u128 << 96, 32), // 2001:db8::/32 documentation
    (0x2002_u128 << 112, 16),     // 2002::/16 6to4
    (0x3fff_0000_u128 << 96, 20), // 3fff::/20 documentation
];

fn in_prefix<T>(address: T, network: T, length: u32, bits: u32) -> bool
where
    T: Copy + PartialEq + std::ops::Shr<u32, Output = T>,
{
    length == 0 || address >> (bits - length) == network >> (bits - length)
}

pub fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let ip = u32::from(ip);
            !NON_PUBLIC_V4
                .iter()
                .any(|&(network, length)| in_prefix(ip, network, length, 32))
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            // ::ffff:a.b.c.d reaches the embedded IPv4 address directly.
            Some(ip) => public_ip(IpAddr::V4(ip)),
            None => {
                let ip = u128::from(ip);
                in_prefix(ip, 0x2000_u128 << 112, 3, 128)
                    && !NON_PUBLIC_V6
                        .iter()
                        .any(|&(network, length)| in_prefix(ip, network, length, 128))
            }
        },
    }
}
