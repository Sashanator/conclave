//! Templates shipped with Conclave. They are copied into the user's templates folder once (see store::seed_templates_in).

pub const STANDARD_MEETING: &str = include_str!("../../../templates/standard_meeting.json");

pub const SEED_TEMPLATES: [(&str, &str); 8] = [
    ("standard_meeting", STANDARD_MEETING),
    ("german_lessons", include_str!("../../../templates/german_lessons.json")),
    ("one_two_one_rasulov", include_str!("../../../templates/one_two_one_rasulov.json")),
    ("rasulov_lpr", include_str!("../../../templates/rasulov_lpr.json")),
    ("portfolio_meetings", include_str!("../../../templates/portfolio_meetings.json")),
    ("scrum_of_scrums", include_str!("../../../templates/scrum_of_scrums.json")),
    ("delivery_management", include_str!("../../../templates/delivery_management.json")),
    ("one_to_one", include_str!("../../../templates/one_to_one.json")),
];

/// Added in a later release; seeded once more into existing installs (`.seeded-v2`), missing files only.
pub const SEED_TEMPLATES_V2: [(&str, &str); 1] = [("interview", include_str!("../../../templates/interview.json"))];
