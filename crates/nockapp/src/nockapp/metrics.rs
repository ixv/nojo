use gnort::*;

metrics_struct![
    NockAppMetrics,
    (handle_shutdown, "nockapp.handle_shutdown", Count),
    (handle_save_permit_res, "nockapp.handle_save_permit_res", Count),
    (handle_action, "nockapp.handle_action", Count),
    (handle_exit, "nockapp.handle_exit", Count),
    (poke_during_exit, "nockapp.poke_during_exit", Count),
    (peek_during_exit, "nockapp.peek_during_exit", Count)
];
