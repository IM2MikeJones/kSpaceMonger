fn main() {
    let host = std::env::args().nth(1).expect("usage: remote_smoke <ssh-host>");
    let p = kspacemonger::remote::ssh_probe(&host).unwrap_or_else(|e| {
        eprintln!("{}", e.user_message());
        std::process::exit(1);
    });
    println!("probe: {:?} · {} · store={}", p.os, p.label, if p.home_ok { "home" } else { "temp" });
    let path = kspacemonger::remote::ensure_agent(&host, &p).unwrap_or_else(|e| {
        eprintln!("{}", e.user_message());
        std::process::exit(1);
    });
    println!("agent: {path}");
}
