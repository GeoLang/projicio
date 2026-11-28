TEST(operation, projCRS_to_projCRS_context_incompatible_areas) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("32631"), // UTM 31
        authFactory->createCoordinateReferenceSystem("32633"), // UTM 33
        ctxt);
    ASSERT_EQ(list.size(), 1U);
    EXPECT_EQ(list[0]->nameStr(), "Inverse of UTM zone 31N + UTM zone 33N");
    ASSERT_EQ(list[0]->coordinateOperationAccuracies().size(), 1U);
    EXPECT_EQ(list[0]->coordinateOperationAccuracies()[0]->value(), "0");
}

