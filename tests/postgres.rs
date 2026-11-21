use std::{sync::Arc, time::Duration};

use arena_core::{
    domain::{
        game_match::MatchMode,
        lobby::{Lobby, LobbyStatus},
        matchmaking::{MatchmakingConfig, PlayerMatchmakingStatus, QueueEntry},
        player::Player,
    },
    storage::{
        postgres::PostgresRepository, LobbyRepository, LobbyRepositoryError, MatchmakingRepository,
        MatchmakingRepositoryError, PlayerRepository,
    },
};
use tokio::task::JoinSet;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_repositories_preserve_invariants_and_history() {
    let Ok(database_url) = std::env::var("ARENA_TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration assertions; ARENA_TEST_DATABASE_URL is unset");
        return;
    };
    let repository = Arc::new(
        PostgresRepository::connect(&database_url, 10, Duration::from_secs(5))
            .await
            .unwrap(),
    );
    sqlx::query(
        "TRUNCATE match_history, match_players, matches, matchmaking_queue, lobby_memberships, lobbies, player_activities, players RESTART IDENTITY CASCADE",
    )
    .execute(repository.pool())
    .await
    .unwrap();

    let owner = insert_player(&repository, "Postgres Owner", 1200).await;
    let lobby = Lobby::new(owner.id(), 2);
    let lobby = LobbyRepository::create(&*repository, lobby).await.unwrap();
    let mut candidates = Vec::new();
    for index in 0..6 {
        candidates.push(
            insert_player(
                &repository,
                &format!("Database Joiner {index}"),
                1200 + index,
            )
            .await,
        );
    }

    let mut joins = JoinSet::new();
    for candidate in &candidates {
        let repository = Arc::clone(&repository);
        let lobby_id = lobby.id();
        let player_id = candidate.id();
        joins.spawn(async move { LobbyRepository::join(&*repository, lobby_id, player_id).await });
    }
    let mut joined_player = None;
    let mut full_rejections = 0;
    while let Some(result) = joins.join_next().await {
        match result.unwrap() {
            Ok(updated) => {
                joined_player = updated
                    .members()
                    .iter()
                    .find(|member| member.player_id() != owner.id())
                    .map(|member| member.player_id());
            }
            Err(LobbyRepositoryError::Lobby(arena_core::domain::lobby::LobbyError::Full)) => {
                full_rejections += 1;
            }
            Err(error) => panic!("unexpected PostgreSQL join error: {error}"),
        }
    }
    assert_eq!(full_rejections, 5);
    let joined_player = joined_player.expect("one player should claim the final lobby slot");
    LobbyRepository::set_ready(&*repository, lobby.id(), owner.id(), true)
        .await
        .unwrap();
    let ready_lobby = LobbyRepository::set_ready(&*repository, lobby.id(), joined_player, true)
        .await
        .unwrap();
    assert_eq!(ready_lobby.status(), LobbyStatus::Ready);
    assert_eq!(ready_lobby.members().len(), 2);

    let conflict = MatchmakingRepository::enqueue(
        &*repository,
        QueueEntry::new(owner.id(), owner.rating(), MatchMode::OneVsOne, 0),
        0,
        MatchmakingConfig::default(),
    )
    .await;
    assert!(matches!(
        conflict,
        Err(MatchmakingRepositoryError::PlayerInLobby)
    ));

    let first = insert_player(&repository, "Persistent Duelist One", 1300).await;
    let second = insert_player(&repository, "Persistent Duelist Two", 1350).await;
    let first_result = MatchmakingRepository::enqueue(
        &*repository,
        QueueEntry::new(first.id(), first.rating(), MatchMode::OneVsOne, 10),
        10,
        MatchmakingConfig::default(),
    )
    .await
    .unwrap();
    assert!(matches!(
        first_result.status,
        PlayerMatchmakingStatus::Queued {
            mode: MatchMode::OneVsOne
        }
    ));
    let second_result = MatchmakingRepository::enqueue(
        &*repository,
        QueueEntry::new(second.id(), second.rating(), MatchMode::OneVsOne, 11),
        11,
        MatchmakingConfig::default(),
    )
    .await
    .unwrap();
    assert_eq!(second_result.formed_matches.len(), 1);
    let formed_match = &second_result.formed_matches[0];
    let stored_match = MatchmakingRepository::find_match(&*repository, formed_match.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_match, *formed_match);

    let history_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM match_history WHERE match_id = $1")
            .bind(formed_match.id())
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(history_count, 1);

    let reconnected = PostgresRepository::connect(&database_url, 2, Duration::from_secs(5))
        .await
        .unwrap();
    let restored_player = PlayerRepository::find_by_id(&reconnected, first.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored_player, first);
    assert!(matches!(
        MatchmakingRepository::status(&reconnected, first.id())
            .await
            .unwrap(),
        PlayerMatchmakingStatus::Matched { .. }
    ));
}

async fn insert_player(repository: &PostgresRepository, display_name: &str, rating: u16) -> Player {
    let player = Player::new(display_name.to_owned(), rating);
    PlayerRepository::insert(repository, player).await.unwrap()
}
